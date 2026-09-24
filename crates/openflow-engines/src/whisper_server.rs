//! OWNER: team C (engines). The meeting speech server on port 8768 (WhisperServer.swift, moved to transcribe.cpp).
//!
//! `transcribe-server` (sidecar/transcribe-server: transcribe.cpp on Metal, bundled with the app) running Whisper
//! large v3 turbo (GGUF Q8_0): multilingual with reliable language detection, and much better than Parakeet on
//! compressed call audio. About half the memory and twice the speed of Homebrew's whisper-server, which still
//! works when `whisperServerPath` points at it. Started for the length of a meeting, then stopped to free ~1 GB.
//! Requests carry a per-launch bearer token; the server exits by itself if the app dies.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use openflow_core::whisper_filter::filter_whisper;
use openflow_core::Config;
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::watch;

use crate::cloud::{multipart_body, new_boundary, segments_of};
use crate::polisher::{pkill, terminate};

pub const PORT: u16 = 8768;

/// A meeting restarts a crashed server at most this many times, then stays on Parakeet.
const MAX_RESTARTS: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Idle,
    Starting,
    Ready,
    Failed,
}

pub struct WhisperServer {
    config: Config,
    base: String,
    child: Mutex<Option<Child>>,
    http: reqwest::Client,
    status: watch::Sender<Status>,
    restarts: AtomicU32,
    /// Bearer token for this server instance (transcribe-server rejects requests without it).
    token: String,
}

pub const SERVER_NAME: &str = "transcribe-server";

/// Where the bundled transcribe-server is: `$OPENFLOW_TRANSCRIBE_SERVER`, else next to the app's executable (Tauri
/// externalBin, with or without the target-triple suffix), else the workspace build (dev).
pub fn bundled_server_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("OPENFLOW_TRANSCRIBE_SERVER").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        let triple = format!("{}-apple-darwin", std::env::consts::ARCH);
        for name in [SERVER_NAME.to_string(), format!("{SERVER_NAME}-{triple}")] {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sidecar/transcribe-server").join(SERVER_NAME);
    dev.is_file().then_some(dev)
}

/// `whisperServerPath` when set, else the bundled transcribe-server.
pub fn resolve_server_path(config: &Config) -> Option<PathBuf> {
    if config.whisper_server_path.is_empty() {
        bundled_server_path()
    } else {
        Some(PathBuf::from(&config.whisper_server_path))
    }
}

/// Homebrew's whisper-server takes different flags and no token.
fn is_legacy(path: &Path) -> bool {
    path.file_name().is_some_and(|n| n.to_string_lossy().starts_with("whisper-server"))
}

/// Arguments after `transcribe-server`.
pub fn server_args(model_path: &str) -> Vec<String> {
    ["-m", model_path, "--host", "127.0.0.1", "--port", &PORT.to_string()].iter().map(|s| s.to_string()).collect()
}

/// Arguments after Homebrew's `whisper-server`. `-nf`: no temperature fallback, which is where Whisper's fluent inventions on
/// noise come from; `-sns`: suppress non-speech tokens ("[Music]", "♪").
pub fn whisper_args(model_path: &str) -> Vec<String> {
    ["-m", model_path, "--host", "127.0.0.1", "--port", &PORT.to_string(), "-l", "auto", "-t", "6", "-nf", "-sns"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// The dictionary's terms as Whisper's initial prompt, so names and jargon are spelled right. Capped: a long
/// prompt eats the 224-token context and makes Whisper echo it on quiet audio.
pub fn vocabulary_prompt(dictionary: &[String]) -> String {
    let mut out = String::new();
    for term in openflow_core::text_rules::dictionary_terms(dictionary) {
        if out.len() + term.len() > 400 {
            break;
        }
        out += if out.is_empty() { "Vocabulary: " } else { ", " };
        out += &term;
    }
    if !out.is_empty() {
        out.push('.');
    }
    out
}

/// On noise Whisper can "transcribe" its own prompt ("Vocabulary.", "Zentro, Priya."): output whose words all come
/// from the prompt is dropped.
pub fn echoes_prompt(text: &str, prompt: &str) -> bool {
    let words = |t: &str| -> Vec<String> {
        t.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect()
    };
    let said = words(text);
    let prompt = words(prompt);
    !said.is_empty() && !prompt.is_empty() && said.iter().all(|w| prompt.contains(w))
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
            status: watch::channel(Status::Idle).0,
            restarts: AtomicU32::new(0),
            token: uuid::Uuid::new_v4().simple().to_string(),
        }
    }

    /// The configured server, or the bundled transcribe-server.
    pub fn server_path(&self) -> Option<PathBuf> {
        resolve_server_path(&self.config)
    }

    pub fn status(&self) -> Status {
        *self.status.borrow()
    }

    /// Waits while the server is starting (start() itself gives up after 60 s). True when it is ready.
    pub async fn wait_ready(&self) -> bool {
        let mut rx = self.status.subscribe();
        let ready = rx.wait_for(|s| *s != Status::Starting).await.map(|s| *s == Status::Ready).unwrap_or(false);
        ready
    }

    /// After a failed request: when the process has died, start it again (a bounded number of times).
    /// Returns true when a restart was attempted and the server is back.
    pub async fn recover(&self) -> bool {
        let exited = {
            let mut child = self.child.lock().unwrap();
            match child.as_mut().map(|c| c.try_wait()) {
                Some(Ok(Some(_))) | Some(Err(_)) | None => true,
                Some(Ok(None)) => false,
            }
        };
        if !exited || self.status() == Status::Starting {
            return false;
        }
        if self.restarts.fetch_add(1, Ordering::SeqCst) >= MAX_RESTARTS {
            self.status.send_replace(Status::Failed);
            return false;
        }
        tracing::warn!(target: "whisper", "speech server exited; restarting");
        match self.start().await {
            Ok(()) => true,
            Err(e) => {
                tracing::error!(target: "whisper", "speech server restart failed: {e:#}");
                false
            }
        }
    }

    /// Points requests at another server (tests use a local mock).
    pub fn with_base_url(mut self, base: impl Into<String>) -> Self {
        self.base = base.into().trim_end_matches('/').to_string();
        self
    }

    pub fn model_path(&self) -> PathBuf {
        openflow_core::paths::models_dir().join(&self.config.whisper_model)
    }

    /// The binary and the model both exist.
    pub fn available(&self) -> bool {
        self.server_path().is_some_and(|p| p.exists()) && self.model_path().exists()
    }

    /// Kills a leftover server on the port, spawns a new one and waits up to 60 s for it to answer.
    pub async fn start(&self) -> Result<()> {
        if !self.available() {
            self.status.send_replace(Status::Failed);
            bail!("the speech server or its model is missing");
        }
        self.status.send_replace(Status::Starting);
        pkill(&format!("(transcribe|whisper)-server.*--port {PORT}")).await;
        let path = self.server_path().unwrap_or_default();
        let model = self.model_path().to_string_lossy().to_string();
        let args = if is_legacy(&path) { whisper_args(&model) } else { server_args(&model) };
        let mut child = Command::new(&path)
            .args(args)
            .env("OPENFLOW_STT_TOKEN", &self.token)
            .env("OPENFLOW_PARENT_PID", std::process::id().to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .inspect_err(|_| {
                self.status.send_replace(Status::Failed);
            })?;
        if let Some(err) = child.stderr.take() {
            tokio::spawn(async move {
                let mut lines = BufReader::new(err).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::debug!(target: "whisper", "{line}");
                }
            });
        }
        if let Some(old) = self.child.lock().unwrap().replace(child) {
            terminate(old);
        }
        let deadline = Instant::now() + Duration::from_secs(60);
        while Instant::now() < deadline {
            if self.is_up().await {
                tracing::info!(target: "whisper", "speech server ready ({})", path.display());
                self.warm_up().await;
                self.status.send_replace(Status::Ready);
                return Ok(());
            }
            if self.status() == Status::Idle {
                bail!("speech server stopped while starting");
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        self.status.send_replace(Status::Failed);
        bail!("speech server didn't start")
    }

    /// The first request pays for Metal pipeline compilation; spend it on a second of silence, not on speech.
    async fn warm_up(&self) {
        let started = Instant::now();
        let _ = self.request(&vec![0.0; 16_000]).await;
        tracing::info!(target: "whisper", "speech server warm in {} ms", started.elapsed().as_millis());
    }

    pub fn stop(&self) {
        self.status.send_replace(Status::Idle);
        if let Some(child) = self.child.lock().unwrap().take() {
            terminate(child);
        }
    }

    /// `/` answers 200 once the model is loaded (401 would be someone else's server on our port).
    async fn is_up(&self) -> bool {
        let r = self.http.get(format!("{}/", self.base)).bearer_auth(&self.token).timeout(Duration::from_secs(1)).send().await;
        r.is_ok_and(|r| r.status().is_success())
    }

    /// Returns the text and the detected language name (whisper's, lowercased: "english", "chinese").
    pub async fn transcribe(&self, samples: &[f32]) -> Result<(String, String)> {
        let json = self.request(samples).await?;
        let text = json.get("text").and_then(Value::as_str).ok_or_else(|| anyhow!("speech server reply not understood"))?;
        let kept = filter_whisper(&join_lines(text), &segments_of(&json), samples.len() as f64 / 16_000.0);
        let kept = if echoes_prompt(&kept, &vocabulary_prompt(&self.config.dictionary)) { String::new() } else { kept };
        let language = json.get("language").and_then(Value::as_str).unwrap_or_default().to_lowercase();
        Ok((kept, language))
    }

    async fn request(&self, samples: &[f32]) -> Result<Value> {
        let boundary = new_boundary();
        let prompt = vocabulary_prompt(&self.config.dictionary);
        let mut fields = vec![("response_format", "verbose_json"), ("temperature", "0.0"), ("temperature_inc", "0.0")];
        if !prompt.is_empty() {
            fields.push(("prompt", prompt.as_str()));
        }
        let body = multipart_body(&boundary, &fields, &openflow_core::wav::pcm16_bytes(samples));
        let resp = self
            .http
            .post(format!("{}/inference", self.base))
            .bearer_auth(&self.token)
            .header("Content-Type", format!("multipart/form-data; boundary={boundary}"))
            .body(body)
            .timeout(Duration::from_secs(60))
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!("speech server answered {}", resp.status());
        }
        let data = resp.bytes().await?;
        serde_json::from_slice(&data).map_err(|_| anyhow!("speech server reply not understood"))
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
        assert_eq!(whisper_args("/m/g.bin").join(" "), "-m /m/g.bin --host 127.0.0.1 --port 8768 -l auto -t 6 -nf -sns");
    }

    #[test]
    fn prompt_echo() {
        let p = vocabulary_prompt(&["Zentro".into(), "Priya".into()]);
        assert!(echoes_prompt("Vocabulary.", &p));
        assert!(echoes_prompt("Zentro, Priya.", &p));
        assert!(!echoes_prompt("Priya owns the checklist.", &p));
        assert!(!echoes_prompt("Vocabulary.", ""));
        assert!(!echoes_prompt("", &p));
    }

    #[test]
    fn server_selection() {
        assert_eq!(server_args("/m/w.gguf").join(" "), "-m /m/w.gguf --host 127.0.0.1 --port 8768");
        assert!(is_legacy(Path::new("/opt/homebrew/bin/whisper-server")));
        assert!(!is_legacy(Path::new("/Applications/OpenFlow.app/Contents/MacOS/transcribe-server")));
        let c = Config { whisper_server_path: "/custom/whisper-server".into(), ..Config::default() };
        assert_eq!(WhisperServer::new(c).server_path(), Some(PathBuf::from("/custom/whisper-server")));
    }

    #[test]
    fn vocabulary() {
        assert_eq!(vocabulary_prompt(&[]), "");
        assert_eq!(vocabulary_prompt(&["Zentro".into(), "OpenFlow".into()]), "Vocabulary: Zentro, OpenFlow.");
        let many: Vec<String> = (0..200).map(|i| format!("Term{i}")).collect();
        assert!(vocabulary_prompt(&many).len() < 420);
    }

    #[test]
    fn joins_lines() {
        assert_eq!(join_lines(" hello\nworld  and \n\n more"), " hello world and more");
    }
}
