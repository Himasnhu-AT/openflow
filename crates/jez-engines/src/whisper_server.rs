//! OWNER: team C (engines). whisper.cpp server on port 8768 (WhisperServer.swift).
//!
//! whisper.cpp's server (Homebrew `whisper-cpp`) running Whisper large v3 turbo on Metal: multilingual with
//! reliable language detection, and much better than Parakeet on compressed call audio. Started for the length
//! of a meeting, then stopped to free ~2 GB.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use jez_core::whisper_filter::filter_whisper;
use jez_core::Config;
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;
use tokio::process::{Child, Command};

use crate::cloud::{multipart_body, new_boundary, segments_of};
use crate::polisher::{pkill, terminate};

pub const PORT: u16 = 8768;

pub struct WhisperServer {
    config: Config,
    base: String,
    child: Mutex<Option<Child>>,
    http: reqwest::Client,
}

/// Arguments after `whisper-server`, exactly as WhisperServer.swift passes them.
pub fn whisper_args(model_path: &str) -> Vec<String> {
    ["-m", model_path, "--host", "127.0.0.1", "--port", &PORT.to_string(), "-l", "auto", "-t", "6"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// Newlines become spaces, runs of whitespace collapse to one space.
pub fn join_lines(text: &str) -> String {
    static WS: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s{2,}").unwrap());
    WS.replace_all(&text.replace('\n', " "), " ").into_owned()
}

impl WhisperServer {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            base: format!("http://127.0.0.1:{PORT}"),
            child: Mutex::new(None),
            http: reqwest::Client::new(),
        }
    }

    /// Points requests at another server (tests use a local mock).
    pub fn with_base_url(mut self, base: impl Into<String>) -> Self {
        self.base = base.into().trim_end_matches('/').to_string();
        self
    }

    pub fn model_path(&self) -> PathBuf {
        jez_core::paths::models_dir().join(&self.config.whisper_model)
    }

    /// The binary and the model both exist.
    pub fn available(&self) -> bool {
        std::path::Path::new(&self.config.whisper_server_path).exists() && self.model_path().exists()
    }

    /// Kills a leftover server on the port, spawns a new one and waits up to 60 s for it to answer.
    pub async fn start(&self) -> Result<()> {
        if !self.available() {
            bail!("whisper-server or its model is missing");
        }
        pkill(&format!("whisper-server.*--port {PORT}")).await;
        let child = Command::new(&self.config.whisper_server_path)
            .args(whisper_args(&self.model_path().to_string_lossy()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        if let Some(old) = self.child.lock().unwrap().replace(child) {
            terminate(old);
        }
        let deadline = Instant::now() + Duration::from_secs(60);
        while Instant::now() < deadline {
            if self.is_up().await {
                tracing::info!(target: "whisper", "whisper-server ready");
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        bail!("whisper-server didn't start")
    }

    pub fn stop(&self) {
        if let Some(child) = self.child.lock().unwrap().take() {
            terminate(child);
        }
    }

    /// Any HTTP response on `/` counts.
    async fn is_up(&self) -> bool {
        self.http.get(format!("{}/", self.base)).timeout(Duration::from_secs(1)).send().await.is_ok()
    }

    /// Returns the text and the detected language name (whisper's, lowercased: "english", "chinese").
    pub async fn transcribe(&self, samples: &[f32]) -> Result<(String, String)> {
        let boundary = new_boundary();
        let body = multipart_body(
            &boundary,
            &[("response_format", "verbose_json"), ("temperature", "0.0"), ("temperature_inc", "0.2")],
            &jez_core::wav::pcm16_bytes(samples),
        );
        let resp = self
            .http
            .post(format!("{}/inference", self.base))
            .header("Content-Type", format!("multipart/form-data; boundary={boundary}"))
            .body(body)
            .timeout(Duration::from_secs(60))
            .send()
            .await?;
        let data = resp.bytes().await?;
        let json: Value = serde_json::from_slice(&data).map_err(|_| anyhow!("whisper-server reply not understood"))?;
        let text = json.get("text").and_then(Value::as_str).ok_or_else(|| anyhow!("whisper-server reply not understood"))?;
        let kept = filter_whisper(&join_lines(text), &segments_of(&json), samples.len() as f64 / 16_000.0);
        let language = json.get("language").and_then(Value::as_str).unwrap_or_default().to_lowercase();
        Ok((kept, language))
    }
}

impl Drop for WhisperServer {
    fn drop(&mut self) {
        if let Some(child) = self.child.get_mut().ok().and_then(Option::take) {
            terminate(child);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_exact() {
        assert_eq!(whisper_args("/m/g.bin").join(" "), "-m /m/g.bin --host 127.0.0.1 --port 8768 -l auto -t 6");
    }

    #[test]
    fn joins_lines() {
        assert_eq!(join_lines(" hello\nworld  and \n\n more"), " hello world and more");
    }
}
