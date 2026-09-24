//! OWNER: team C (engines). Model downloads with progress/cancel (ModelCatalog.swift ModelDownloader).
//!
//! Downloads are keyed by file name. A file already downloading is ignored. Bytes stream into
//! `<dest>/<file>.download`, which replaces `<dest>/<file>` when complete. Multi-GB files from Hugging Face stall
//! for more than a minute at times, so a download only gives up after 10 minutes without data (and a day overall).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{bail, Result};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::task::AbortHandle;

/// One progress report.
/// - `progress`: `Some(0.0)` at start, then `Some(fraction)` while running (only when the size is known).
/// - `done`: finished; `error` is set when it failed. Cancelled downloads report nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct DownloadEvent {
    pub file: String,
    pub progress: Option<f64>,
    pub done: bool,
    pub error: Option<String>,
}

pub type ProgressFn = Arc<dyn Fn(DownloadEvent) + Send + Sync>;

pub struct Downloader {
    http: reqwest::Client,
    tasks: Arc<Mutex<HashMap<String, AbortHandle>>>,
    on_event: ProgressFn,
}

/// Swift keys downloads by the URL's last path component.
pub fn file_name_of(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    path.trim_end_matches('/').rsplit('/').next().unwrap_or_default().to_string()
}

impl Downloader {
    pub fn new(on_event: ProgressFn) -> Self {
        let http = reqwest::Client::builder()
            .read_timeout(Duration::from_secs(600))
            .timeout(Duration::from_secs(86_400))
            .build()
            .unwrap_or_default();
        Self { http, tasks: Arc::new(Mutex::new(HashMap::new())), on_event }
    }

    pub fn is_downloading(&self, file: &str) -> bool {
        self.tasks.lock().unwrap().contains_key(file)
    }

    /// Starts downloading `url` into `dest_dir/file`. Returns false (and does nothing) when `file` is already
    /// downloading. Must be called inside a tokio runtime.
    pub fn start(&self, file: &str, url: &str, dest_dir: &Path) -> bool {
        let mut tasks = self.tasks.lock().unwrap();
        if tasks.contains_key(file) {
            return false;
        }
        (self.on_event)(DownloadEvent { file: file.into(), progress: Some(0.0), done: false, error: None });
        let (http, on_event, all) = (self.http.clone(), self.on_event.clone(), self.tasks.clone());
        let (file_s, url_s, dest) = (file.to_string(), url.to_string(), dest_dir.join(file));
        let handle = tokio::spawn(async move {
            let result = fetch(&http, &url_s, &dest, &file_s, &on_event).await;
            all.lock().unwrap().remove(&file_s);
            on_event(DownloadEvent {
                file: file_s,
                progress: None,
                done: true,
                error: result.err().map(|e| e.to_string()),
            });
        });
        tasks.insert(file.to_string(), handle.abort_handle());
        true
    }

    /// Stops a download and removes its partial file. Nothing is reported.
    pub fn cancel(&self, file: &str, dest_dir: &Path) {
        if let Some(h) = self.tasks.lock().unwrap().remove(file) {
            h.abort();
            let _ = std::fs::remove_file(temp_path(&dest_dir.join(file)));
        }
    }
}

impl Drop for Downloader {
    fn drop(&mut self) {
        for (_, h) in self.tasks.lock().unwrap().drain() {
            h.abort();
        }
    }
}

fn temp_path(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(".download");
    dest.with_file_name(name)
}

async fn fetch(http: &reqwest::Client, url: &str, dest: &Path, file: &str, on_event: &ProgressFn) -> Result<()> {
    if let Some(dir) = dest.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    let resp = http.get(url).send().await?;
    if !resp.status().is_success() {
        bail!("Download failed: HTTP {}", resp.status().as_u16());
    }
    let total = resp.content_length().filter(|n| *n > 0);
    let tmp = temp_path(dest);
    let mut out = tokio::fs::File::create(&tmp).await?;
    let mut written: u64 = 0;
    let mut hasher = Sha256::new();
    let mut stream = resp.bytes_stream();
    let mut last_reported = -1.0f64;
    let result: Result<()> = async {
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            out.write_all(&chunk).await?;
            hasher.update(&chunk);
            written += chunk.len() as u64;
            if let Some(total) = total {
                let p = written as f64 / total as f64;
                // Report at most every 0.1 % so the UI isn't flooded.
                if p - last_reported >= 0.001 || written == total {
                    last_reported = p;
                    on_event(DownloadEvent { file: file.into(), progress: Some(p), done: false, error: None });
                }
            }
        }
        out.flush().await?;
        // Pinned models are checked byte for byte: a truncated or swapped file must never be loaded.
        if let Some(want) = openflow_core::catalog::expected_sha256(file) {
            let got = format!("{:x}", hasher.finalize());
            if got != want {
                bail!("Download failed: checksum mismatch for {file}");
            }
        }
        Ok(())
    }
    .await;
    drop(out);
    if let Err(e) = result {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(e);
    }
    let _ = tokio::fs::remove_file(dest).await;
    tokio::fs::rename(&tmp, dest).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(
            file_name_of("https://huggingface.co/unsloth/Qwen3.5-2B-GGUF/resolve/main/Qwen3.5-2B-Q4_K_M.gguf"),
            "Qwen3.5-2B-Q4_K_M.gguf"
        );
        assert_eq!(file_name_of("https://x/a/b.bin?download=true"), "b.bin");
        assert_eq!(temp_path(Path::new("/m/x.gguf")), PathBuf::from("/m/x.gguf.download"));
    }
}
