//! PolishBackend impl (Polisher.swift): cleans up transcripts with a local LLM.
//!
//! The engine is Ollama by default (`config.llm_backend`), or llama-server run as a child process with a GGUF
//! from models/. Prompts are identical for both. When the cloud applies (see `Cloud::new`), completions go
//! to Groq first and fall back to the local engine.

use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use openflow_core::field_context::FieldContext;
use openflow_core::traits::{Examples, PolishBackend};
use openflow_core::Config;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use tokio::process::{Child, Command};

use crate::cloud::{cut_think, Cloud, Scope};
use crate::notes::Completer;
use crate::ollama::Ollama;
use crate::prompts::{self, trim_ws_nl};

/// Arguments after `llama-server`, exactly as Polisher.swift passes them.
pub fn llama_args(model_path: &str, port: u16) -> Vec<String> {
    [
        "-m",
        model_path,
        "--host",
        "127.0.0.1",
        "--port",
        &port.to_string(),
        "-ngl",
        "99",
        "-c",
        "4096",
        "-np",
        "1",
        "--reasoning-budget",
        "0",
        "--reasoning-format",
        "none",
        "--chat-template-kwargs",
        "{\"enable_thinking\":false}",
        "--no-webui",
        "--log-disable",
        // Prompt-lookup speculative decoding: cleanup output mostly copies the transcript, so drafts from the
        // prompt's own n-grams are accepted nearly every time (measured 27 -> 90+ tok/s on the M4).
        "--spec-type",
        "ngram-mod",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Runs `pkill -f <pattern>` and waits for it (errors ignored): clears a server left over from an earlier run.
pub(crate) async fn pkill(pattern: &str) {
    let bin = if std::path::Path::new("/usr/bin/pkill").exists() { "/usr/bin/pkill" } else { "pkill" };
    let _ = Command::new(bin)
        .args(["-f", pattern])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;
}

/// Sends SIGTERM (Swift `Process.terminate()`) and reaps the child in the background.
pub(crate) fn terminate(mut child: Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        // SAFETY: plain kill(2) on a pid we spawned and have not reaped yet.
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGTERM);
        }
    }
    #[cfg(not(unix))]
    let _ = child.start_kill();
    match tokio::runtime::Handle::try_current() {
        Ok(h) => {
            h.spawn(async move {
                let _ = child.wait().await;
            });
        }
        Err(_) => {
            let _ = child.start_kill();
        }
    }
}

/// Qwen3.5 emits an empty `<think></think>` block first even with thinking off; only show what follows it.
/// Returns "" while still inside the think block.
pub fn strip_think(s: &str) -> String {
    if let Some(i) = s.find("</think>") {
        trim_ws_nl(&s[i + "</think>".len()..]).to_string()
    } else if s.contains("<think>") {
        String::new()
    } else {
        trim_ws_nl(s).to_string()
    }
}

/// What one SSE line meant.
#[derive(Debug, PartialEq)]
pub enum SseStep {
    /// Not a data line, unparseable, or no content.
    Skip,
    /// New content arrived; `visible` is the think-stripped text so far (may be empty).
    Content { visible: String },
    /// `data: [DONE]`.
    Done,
}

/// Accumulates an OpenAI-style SSE stream from llama-server (`Polisher.stream`).
#[derive(Debug, Default)]
pub struct SseAccumulator {
    pub text: String,
    pub chunks: usize,
    /// (prompt_n, prompt_ms, predicted_n, predicted_ms) of the last `timings` object seen.
    pub timings: Option<(i64, f64, i64, f64)>,
    /// The last fed line carried a `timings` object.
    pub timings_updated: bool,
}

impl SseAccumulator {
    pub fn feed_line(&mut self, line: &str) -> SseStep {
        self.timings_updated = false;
        let Some(payload) = line.strip_prefix("data: ") else { return SseStep::Skip };
        if payload == "[DONE]" {
            return SseStep::Done;
        }
        let Ok(json) = serde_json::from_str::<Value>(payload) else { return SseStep::Skip };
        if let Some(t) = json.get("timings").filter(|t| t.is_object()) {
            self.timings = Some(timings_of(t));
            self.timings_updated = true;
        }
        let Some(piece) = json.pointer("/choices/0/delta/content").and_then(Value::as_str) else {
            return SseStep::Skip;
        };
        if piece.is_empty() {
            return SseStep::Skip;
        }
        self.chunks += 1;
        self.text.push_str(piece);
        SseStep::Content { visible: strip_think(&self.text) }
    }

    pub fn result(&self) -> String {
        strip_think(&self.text)
    }
}

fn timings_of(t: &Value) -> (i64, f64, i64, f64) {
    (
        t.get("prompt_n").and_then(Value::as_i64).unwrap_or(0),
        t.get("prompt_ms").and_then(Value::as_f64).unwrap_or(0.0),
        t.get("predicted_n").and_then(Value::as_i64).unwrap_or(0),
        t.get("predicted_ms").and_then(Value::as_f64).unwrap_or(0.0),
    )
}

/// Request body for llama-server's /v1/chat/completions.
pub fn local_body(system: &str, user: &str, max_tokens: usize, stream: bool) -> Value {
    json!({
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
        "temperature": 0.0,
        "max_tokens": max_tokens,
        "stream": stream,
        "cache_prompt": true,
        "chat_template_kwargs": {"enable_thinking": false},
    })
}

pub struct Polisher {
    /// Swappable so settings edits (cloud, learned signals, memory) apply without restarting the engine.
    config: std::sync::RwLock<Config>,
    ollama: std::sync::RwLock<Ollama>,
    scope: Scope,
    current_is_long: AtomicBool,
    child: Mutex<Option<Child>>,
    http: reqwest::Client,
    /// No response caching, no compression (gzip would buffer the whole reply); 60 s between bytes.
    streaming: reqwest::Client,
    first_chunk_ms: AtomicU64,
    last_timings: Mutex<String>,
}

impl Polisher {
    pub fn new(config: Config, scope: Scope) -> Self {
        let streaming = reqwest::Client::builder()
            .read_timeout(Duration::from_secs(60))
            .build()
            .unwrap_or_default();
        Self {
            ollama: std::sync::RwLock::new(Ollama::new(&config.ollama_url)),
            config: std::sync::RwLock::new(config),
            scope,
            current_is_long: AtomicBool::new(false),
            child: Mutex::new(None),
            http: reqwest::Client::new(),
            streaming,
            first_chunk_ms: AtomicU64::new(0),
            last_timings: Mutex::new(String::new()),
        }
    }

    pub fn config(&self) -> Config {
        self.config.read().unwrap().clone()
    }

    /// Replaces the settings used per request. The server keeps running (model and port changes need a restart).
    pub fn set_config(&self, config: Config) {
        if config.ollama_url != self.config.read().unwrap().ollama_url {
            *self.ollama.write().unwrap() = Ollama::new(&config.ollama_url);
        }
        *self.config.write().unwrap() = config;
    }

    fn ollama(&self) -> Option<(Ollama, String)> {
        let c = self.config.read().unwrap();
        c.uses_ollama().then(|| (self.ollama.read().unwrap().clone(), c.ollama_model.clone()))
    }

    /// How long Ollama keeps the model loaded after a call: long for dictation (every press should be
    /// instant), short for the bigger meeting-notes model.
    fn keep_alive(&self) -> &'static str {
        match self.scope {
            Scope::Dictation => "30m",
            Scope::Meetings => "5m",
        }
    }

    fn record(&self, s: String) {
        tracing::info!(target: "polish", "LLM: {s}");
        *self.last_timings.lock().unwrap() = s;
    }

    fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.config.read().unwrap().llama_port)
    }

    /// Set before polishing a long dictation (over 15 s), for the "long dictations" cloud mode.
    pub fn set_current_is_long(&self, long: bool) {
        self.current_is_long.store(long, Ordering::SeqCst);
    }

    fn cloud(&self) -> Option<Cloud> {
        Cloud::new(&self.config.read().unwrap(), self.scope, self.current_is_long.load(Ordering::SeqCst))
    }

    /// Timings of the last local completion, for logs and `--polishtest`.
    pub fn last_timings(&self) -> String {
        self.last_timings.lock().unwrap().clone()
    }

    /// Ollama: makes sure the server runs (starting it if needed) and the model is pulled.
    /// llama.cpp: kills any server left on our port (a previous run or model, so the new model always wins),
    /// then spawns llama-server. Does not wait for it to load; see `wait_until_ready`.
    pub async fn start(&self) -> Result<()> {
        let config = self.config();
        if let Some((ollama, model)) = self.ollama() {
            if let Some(child) = ollama.ensure_running(&config.ollama_path).await? {
                if let Some(old) = self.child.lock().unwrap().replace(child) {
                    terminate(old);
                }
            }
            if !ollama.has_model(&model).await? {
                bail!("Ollama model {model} isn't downloaded. Pull it on the Models page, or run `ollama pull {model}`.");
            }
            return Ok(());
        }
        pkill(&format!("llama-server.*--port {}", config.llama_port)).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        let model = config.model_path();
        if !model.exists() {
            bail!("Model not found: {}", model.display());
        }
        let child = Command::new(&config.llama_server_path)
            .args(llama_args(&model.to_string_lossy(), config.llama_port))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        tracing::info!(target: "polish", "llama-server started pid {}", child.id().unwrap_or(0));
        if let Some(old) = self.child.lock().unwrap().replace(child) {
            terminate(old);
        }
        Ok(())
    }

    pub fn stop(&self) {
        if let Some(child) = self.child.lock().unwrap().take() {
            terminate(child);
        }
    }

    /// Ollama: `/api/version` answers. llama.cpp: `GET /health` with a 1 s timeout, expecting 200.
    pub async fn is_healthy(&self) -> bool {
        if let Some((ollama, _)) = self.ollama() {
            return ollama.is_running().await;
        }
        match self.http.get(format!("{}/health", self.base_url())).timeout(Duration::from_secs(1)).send().await {
            Ok(r) => r.status().as_u16() == 200,
            Err(_) => false,
        }
    }

    /// Polls /health every 500 ms until the model is loaded (Swift default timeout: 120 s).
    pub async fn wait_until_ready(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.is_healthy().await {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        false
    }

    /// Streams tokens as they are generated (SSE) so the app can type them straight into the field.
    pub async fn stream(&self, system: &str, user: &str, max_tokens: usize, on_partial: &(dyn for<'s> Fn(&'s str) + Send + Sync)) -> Result<String> {
        if let Some((ollama, model)) = self.ollama() {
            let (text, t) = ollama
                .chat(&model, system, user, max_tokens, self.keep_alive(), Duration::from_secs(120), Some(on_partial))
                .await?;
            self.record(t.describe());
            return Ok(text);
        }
        let resp = self
            .streaming
            .post(format!("{}/v1/chat/completions", self.base_url()))
            .header("Content-Type", "application/json")
            .header("Accept-Encoding", "identity")
            .body(serde_json::to_vec(&local_body(system, user, max_tokens, true))?)
            .send()
            .await?;
        let started = Instant::now();
        let mut acc = SseAccumulator::default();
        let mut bytes = resp.bytes_stream();
        let mut buf: Vec<u8> = Vec::new();
        'outer: while let Some(chunk) = bytes.next().await {
            buf.extend_from_slice(&chunk?);
            while let Some(nl) = buf.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = buf.drain(..=nl).collect();
                let line = String::from_utf8_lossy(&line);
                if self.handle_line(&mut acc, line.trim_end_matches(['\n', '\r']), started, on_partial) {
                    break 'outer;
                }
            }
        }
        if !buf.is_empty() {
            let line = String::from_utf8_lossy(&buf).to_string();
            self.handle_line(&mut acc, line.trim_end_matches(['\n', '\r']), started, on_partial);
        }
        Ok(acc.result())
    }

    /// Returns true on `[DONE]`.
    fn handle_line(&self, acc: &mut SseAccumulator, line: &str, started: Instant, on_partial: &(dyn for<'s> Fn(&'s str) + Send + Sync)) -> bool {
        let step = acc.feed_line(line);
        if acc.timings_updated {
            if let Some((pn, pms, gn, gms)) = acc.timings {
                let s = format!(
                    "prompt {pn} tok in {} ms, generated {gn} tok in {} ms, first chunk after {} ms",
                    pms as i64,
                    gms as i64,
                    self.first_chunk_ms.load(Ordering::Relaxed)
                );
                tracing::info!(target: "polish", "LLM: {s}");
                *self.last_timings.lock().unwrap() = s;
            }
        }
        match step {
            SseStep::Done => true,
            SseStep::Skip => false,
            SseStep::Content { visible } => {
                if acc.chunks == 1 {
                    self.first_chunk_ms.store(started.elapsed().as_millis() as u64, Ordering::Relaxed);
                }
                if !visible.is_empty() {
                    on_partial(&visible);
                }
                false
            }
        }
    }

    /// One non-streaming completion: cloud first when it applies (falling back to local on any error), then
    /// the local llama-server.
    pub async fn complete(&self, system: &str, user: &str, max_tokens: usize, timeout: Duration) -> Result<String> {
        if let Some(cloud) = self.cloud() {
            match cloud.complete(system, user, max_tokens).await {
                Ok(t) => return Ok(t),
                Err(e) => tracing::error!(target: "polish", "Cloud model failed, using local: {e}"),
            }
        }
        if let Some((ollama, model)) = self.ollama() {
            let (text, t) = ollama.chat(&model, system, user, max_tokens, self.keep_alive(), timeout, None).await?;
            self.record(t.describe());
            return Ok(text);
        }
        let resp = self
            .http
            .post(format!("{}/v1/chat/completions", self.base_url()))
            .header("Content-Type", "application/json")
            .body(serde_json::to_vec(&local_body(system, user, max_tokens, false))?)
            .timeout(timeout)
            .send()
            .await?;
        let data = resp.bytes().await?;
        let json: Value = serde_json::from_slice(&data).map_err(|_| anyhow!("Unexpected llama-server reply"))?;
        let content = json
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("Unexpected llama-server reply"))?;
        if let Some(t) = json.get("timings").filter(|t| t.is_object()) {
            let (pn, pms, gn, gms) = timings_of(t);
            let mut s = format!("prompt {pn} tok in {} ms, generated {gn} tok in {} ms", pms as i64, gms as i64);
            if let Some(acc) = t.get("draft_n_accepted").and_then(Value::as_i64) {
                s += &format!(", draft {acc}/{}", t.get("draft_n").and_then(Value::as_i64).unwrap_or(0));
            }
            tracing::info!(target: "polish", "LLM: {s}");
            *self.last_timings.lock().unwrap() = s;
        }
        Ok(cut_think(content))
    }

    /// Meeting notes (map-reduce) through this polisher.
    pub async fn summarize_meeting(&self, transcript: &str) -> Result<String> {
        let memory = self.config.read().unwrap().memory.clone();
        crate::notes::summarize_meeting(self, transcript, &memory).await
    }

    pub async fn meeting_title(&self, summary: &str) -> Result<String> {
        crate::notes::meeting_title(self, summary).await
    }
}

impl Drop for Polisher {
    fn drop(&mut self) {
        if let Some(child) = self.child.get_mut().ok().and_then(Option::take) {
            terminate(child);
        }
    }
}

#[async_trait]
impl Completer for Polisher {
    async fn complete(&self, system: &str, user: &str, max_tokens: usize, timeout: Duration) -> Result<String> {
        Polisher::complete(self, system, user, max_tokens, timeout).await
    }
}

#[async_trait]
impl PolishBackend for Polisher {
    /// The prompt is laid out so llama-server's prefix cache survives tone switches: the system prompt only
    /// changes when settings change, and the per-dictation tail (tone, field context, transcript) is the user
    /// message.
    async fn polish(
        &self,
        raw: &str,
        tone: &str,
        tones: &BTreeMap<String, String>,
        dictionary: &[String],
        memory: &str,
        examples: &Examples,
        context: Option<&FieldContext>,
        on_partial: Option<&(dyn for<'s> Fn(&'s str) + Send + Sync)>,
    ) -> Result<String> {
        let system = prompts::system_prompt(tones, dictionary, memory, examples, &self.config().learned_signals);
        let user = prompts::user_message(raw, tone, context);
        let content = match on_partial {
            Some(f) if self.cloud().is_none() => self.stream(&system, &user, 1024, f).await?,
            _ => self.complete(&system, &user, 1024, Duration::from_secs(60)).await?,
        };
        Ok(if content.is_empty() { raw.to_string() } else { content })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn think_stripping() {
        assert_eq!(strip_think("<think>"), "");
        assert_eq!(strip_think("<think>\n\n</th"), "");
        assert_eq!(strip_think("<think>\n\n</think>\n\nHello"), "Hello");
        assert_eq!(strip_think("  Hello there \n"), "Hello there");
        assert_eq!(strip_think("a</think>b</think>c"), "b</think>c");
    }

    #[test]
    fn sse_parsing() {
        let mut a = SseAccumulator::default();
        assert_eq!(a.feed_line(""), SseStep::Skip);
        assert_eq!(a.feed_line(": keepalive"), SseStep::Skip);
        assert_eq!(a.feed_line("data: {not json"), SseStep::Skip);
        assert_eq!(a.feed_line(r#"data: {"choices":[{"delta":{"role":"assistant"}}]}"#), SseStep::Skip);
        assert_eq!(a.feed_line(r#"data: {"choices":[{"delta":{"content":""}}]}"#), SseStep::Skip);
        assert_eq!(
            a.feed_line(r#"data: {"choices":[{"delta":{"content":"<think>\n\n"}}]}"#),
            SseStep::Content { visible: String::new() }
        );
        assert_eq!(
            a.feed_line(r#"data: {"choices":[{"delta":{"content":"</think>\n\nHi"}}]}"#),
            SseStep::Content { visible: "Hi".into() }
        );
        assert_eq!(
            a.feed_line(r#"data: {"choices":[{"delta":{"content":" Alex"}}]}"#),
            SseStep::Content { visible: "Hi Alex".into() }
        );
        assert_eq!(
            a.feed_line(r#"data: {"choices":[{"delta":{}}],"timings":{"prompt_n":12,"prompt_ms":30.5,"predicted_n":4,"predicted_ms":40.9}}"#),
            SseStep::Skip
        );
        assert_eq!(a.timings, Some((12, 30.5, 4, 40.9)));
        assert_eq!(a.feed_line("data: [DONE]"), SseStep::Done);
        assert_eq!(a.chunks, 3);
        assert_eq!(a.result(), "Hi Alex");
    }

    #[test]
    fn exact_llama_args() {
        let a = llama_args("/m/x.gguf", 8765).join(" ");
        assert_eq!(
            a,
            "-m /m/x.gguf --host 127.0.0.1 --port 8765 -ngl 99 -c 4096 -np 1 --reasoning-budget 0 --reasoning-format none \
--chat-template-kwargs {\"enable_thinking\":false} --no-webui --log-disable --spec-type ngram-mod"
        );
    }

    #[test]
    fn local_request_body() {
        let b = local_body("s", "u", 1024, true);
        assert_eq!(b["stream"], true);
        assert_eq!(b["cache_prompt"], true);
        assert_eq!(b["chat_template_kwargs"]["enable_thinking"], false);
        assert_eq!(b["max_tokens"], 1024);
        assert_eq!(b["temperature"], 0.0);
    }
}
