//! Ollama: the default local LLM engine. Uses the native API (`/api/chat`, `/api/tags`, `/api/pull`,
//! `/api/version`) because it can switch Qwen's thinking off (`think: false`) and report timings.
//! Prompts are the same as for llama-server; Ollama reuses its KV cache for the unchanged system-prompt prefix.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use futures_util::StreamExt;
use serde::Serialize;
use serde_json::{json, Value};
use tokio::process::{Child, Command};

use crate::cloud::cut_think;

/// Context window requested for every call. Kept constant so Ollama never reloads a model just because a
/// request asked for a different size (the longest request is a ~1300-word meeting chunk plus its notes).
pub const NUM_CTX: u32 = 8192;

/// Where Ollama.app keeps its bundled CLI.
const APP_BINARY: &str = "/Applications/Ollama.app/Contents/Resources/ollama";
const APP_BUNDLE: &str = "/Applications/Ollama.app";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaModel {
    pub name: String,
    /// bytes
    pub size: u64,
}

/// Prompt/generation token counts and durations of the last call, for logs and `--polishtest`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Timings {
    pub prompt_tokens: i64,
    pub prompt_ms: f64,
    pub generated_tokens: i64,
    pub generated_ms: f64,
    pub load_ms: f64,
}

impl Timings {
    fn from_final(v: &Value) -> Timings {
        let ns = |k: &str| v.get(k).and_then(Value::as_f64).unwrap_or(0.0) / 1e6;
        let n = |k: &str| v.get(k).and_then(Value::as_i64).unwrap_or(0);
        Timings {
            prompt_tokens: n("prompt_eval_count"),
            prompt_ms: ns("prompt_eval_duration"),
            generated_tokens: n("eval_count"),
            generated_ms: ns("eval_duration"),
            load_ms: ns("load_duration"),
        }
    }

    pub fn describe(&self) -> String {
        let mut s = format!(
            "prompt {} tok in {} ms, generated {} tok in {} ms",
            self.prompt_tokens, self.prompt_ms as i64, self.generated_tokens, self.generated_ms as i64
        );
        if self.load_ms > 50.0 {
            s += &format!(", model load {} ms", self.load_ms as i64);
        }
        s
    }
}

/// `POST /api/chat` body. `think: false` keeps Qwen from spending tokens on reasoning; temperature 0 matches
/// the llama-server path.
pub fn chat_body(model: &str, system: &str, user: &str, max_tokens: usize, stream: bool, keep_alive: &str, think: bool) -> Value {
    let mut body = json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
        "stream": stream,
        "keep_alive": keep_alive,
        "options": {"temperature": 0, "num_predict": max_tokens, "num_ctx": NUM_CTX},
    });
    if !think {
        body["think"] = json!(false);
    }
    body
}

/// A model tag matches an installed name exactly, or with Ollama's implicit `:latest`.
pub fn tag_matches(installed: &str, wanted: &str) -> bool {
    installed == wanted || (!wanted.contains(':') && installed == format!("{wanted}:latest"))
}

/// The `ollama` CLI: the configured path, Ollama.app's bundled binary, Homebrew/usr-local, or PATH.
pub fn find_binary(configured: &str) -> Option<PathBuf> {
    if !configured.is_empty() {
        let p = PathBuf::from(configured);
        return p.exists().then_some(p);
    }
    for c in [APP_BINARY, "/opt/homebrew/bin/ollama", "/usr/local/bin/ollama"] {
        if Path::new(c).exists() {
            return Some(PathBuf::from(c));
        }
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).map(|d| d.join("ollama")).find(|p| p.exists())
    })
}

pub fn app_installed() -> bool {
    Path::new(APP_BUNDLE).exists()
}

#[derive(Clone)]
pub struct Ollama {
    base: String,
    http: reqwest::Client,
    streaming: reqwest::Client,
}

impl Ollama {
    pub fn new(url: &str) -> Ollama {
        let streaming = reqwest::Client::builder()
            .read_timeout(Duration::from_secs(120))
            .build()
            .unwrap_or_default();
        Ollama { base: url.trim_end_matches('/').to_string(), http: reqwest::Client::new(), streaming }
    }

    pub fn url(&self) -> &str {
        &self.base
    }

    /// `GET /api/version` (1 s timeout). Err when the server isn't reachable.
    pub async fn version(&self) -> Result<String> {
        let v: Value = self
            .http
            .get(format!("{}/api/version", self.base))
            .timeout(Duration::from_secs(1))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(v.get("version").and_then(Value::as_str).unwrap_or_default().to_string())
    }

    pub async fn is_running(&self) -> bool {
        self.version().await.is_ok()
    }

    /// Installed models (`GET /api/tags`).
    pub async fn models(&self) -> Result<Vec<OllamaModel>> {
        let v: Value = self
            .http
            .get(format!("{}/api/tags", self.base))
            .timeout(Duration::from_secs(5))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(v.get("models")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|m| {
                        Some(OllamaModel {
                            name: m.get("name").or_else(|| m.get("model"))?.as_str()?.to_string(),
                            size: m.get("size").and_then(Value::as_u64).unwrap_or(0),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    pub async fn has_model(&self, tag: &str) -> Result<bool> {
        Ok(self.models().await?.iter().any(|m| tag_matches(&m.name, tag)))
    }

    /// One chat completion. Streams (NDJSON) and reports each partial when `on_partial` is set.
    /// Retries once without `think` for models that reject the option.
    #[allow(clippy::too_many_arguments)]
    pub async fn chat(
        &self,
        model: &str,
        system: &str,
        user: &str,
        max_tokens: usize,
        keep_alive: &str,
        timeout: Duration,
        on_partial: Option<&(dyn for<'s> Fn(&'s str) + Send + Sync)>,
    ) -> Result<(String, Timings)> {
        match self.chat_once(model, system, user, max_tokens, keep_alive, timeout, on_partial, false).await {
            Err(e) if e.to_string().contains("does not support thinking") => {
                self.chat_once(model, system, user, max_tokens, keep_alive, timeout, on_partial, true).await
            }
            other => other,
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn chat_once(
        &self,
        model: &str,
        system: &str,
        user: &str,
        max_tokens: usize,
        keep_alive: &str,
        timeout: Duration,
        on_partial: Option<&(dyn for<'s> Fn(&'s str) + Send + Sync)>,
        omit_think: bool,
    ) -> Result<(String, Timings)> {
        let stream = on_partial.is_some();
        // `think` omitted entirely when the model rejects it; otherwise explicitly false.
        let mut body = chat_body(model, system, user, max_tokens, stream, keep_alive, false);
        if omit_think {
            body.as_object_mut().map(|o| o.remove("think"));
        }
        let client = if stream { &self.streaming } else { &self.http };
        let mut req = client.post(format!("{}/api/chat", self.base)).json(&body);
        if !stream {
            req = req.timeout(timeout);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            let msg = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v.get("error").and_then(Value::as_str).map(String::from))
                .unwrap_or(text);
            bail!("Ollama {status}: {msg}");
        }
        if !stream {
            let v: Value = resp.json().await?;
            let content = v
                .pointer("/message/content")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("Unexpected Ollama reply"))?;
            return Ok((cut_think(content), Timings::from_final(&v)));
        }

        let on_partial = on_partial.expect("stream implies a callback");
        let deadline = Instant::now() + timeout;
        let mut text = String::new();
        let mut timings = Timings::default();
        let mut buf: Vec<u8> = Vec::new();
        let mut bytes = resp.bytes_stream();
        while let Some(chunk) = bytes.next().await {
            if Instant::now() > deadline {
                bail!("Ollama timed out");
            }
            buf.extend_from_slice(&chunk?);
            while let Some(nl) = buf.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = buf.drain(..=nl).collect();
                if let Some(done) = feed_ndjson(&line, &mut text, &mut timings, on_partial)? {
                    if done {
                        return Ok((cut_think(&text), timings));
                    }
                }
            }
        }
        if !buf.is_empty() {
            feed_ndjson(&buf, &mut text, &mut timings, on_partial)?;
        }
        Ok((cut_think(&text), timings))
    }

    /// `ollama pull` with progress (0..1, None while the size is unknown). Stops early when `cancel` is set.
    pub async fn pull(&self, model: &str, on_progress: &(dyn Fn(Option<f64>) + Send + Sync), cancel: Arc<AtomicBool>) -> Result<()> {
        let resp = self
            .streaming
            .post(format!("{}/api/pull", self.base))
            .json(&json!({"model": model, "stream": true}))
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!("Ollama {}: {}", resp.status(), resp.text().await.unwrap_or_default());
        }
        let mut buf: Vec<u8> = Vec::new();
        let mut bytes = resp.bytes_stream();
        while let Some(chunk) = bytes.next().await {
            if cancel.load(Ordering::SeqCst) {
                bail!("cancelled");
            }
            buf.extend_from_slice(&chunk?);
            while let Some(nl) = buf.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = buf.drain(..=nl).collect();
                let Ok(v) = serde_json::from_slice::<Value>(&line) else { continue };
                if let Some(err) = v.get("error").and_then(Value::as_str) {
                    bail!("{err}");
                }
                match (v.get("completed").and_then(Value::as_f64), v.get("total").and_then(Value::as_f64)) {
                    (Some(c), Some(t)) if t > 0.0 => on_progress(Some((c / t).min(1.0))),
                    _ => on_progress(None),
                }
                if v.get("status").and_then(Value::as_str) == Some("success") {
                    return Ok(());
                }
            }
        }
        bail!("pull ended before Ollama reported success")
    }

    /// Makes sure the server answers. Starts Ollama.app when installed (it stays running, like the user
    /// would expect), otherwise `ollama serve` as a child the caller owns. Waits up to 20 s.
    pub async fn ensure_running(&self, configured_binary: &str) -> Result<Option<Child>> {
        if self.is_running().await {
            return Ok(None);
        }
        let mut child = None;
        if configured_binary.is_empty() && app_installed() {
            let _ = Command::new("/usr/bin/open").args(["-g", "-a", "Ollama"]).status().await;
        } else {
            let bin = find_binary(configured_binary)
                .ok_or_else(|| anyhow!("Ollama isn't installed. Install it with `brew install ollama` or from ollama.com/download."))?;
            let host = self.base.trim_start_matches("http://").trim_start_matches("https://").to_string();
            child = Some(
                Command::new(bin)
                    .arg("serve")
                    .env("OLLAMA_HOST", host)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .kill_on_drop(true)
                    .spawn()?,
            );
        }
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if self.is_running().await {
                tracing::info!(target: "polish", "Ollama started at {}", self.base);
                return Ok(child);
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
        bail!("Ollama didn't start at {}", self.base)
    }
}

/// One NDJSON line of a streaming `/api/chat`. Returns Some(true) when the reply is complete.
fn feed_ndjson(
    line: &[u8],
    text: &mut String,
    timings: &mut Timings,
    on_partial: &(dyn for<'s> Fn(&'s str) + Send + Sync),
) -> Result<Option<bool>> {
    let line = String::from_utf8_lossy(line);
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }
    let v: Value = serde_json::from_str(line).map_err(|_| anyhow!("Unexpected Ollama stream line"))?;
    if let Some(err) = v.get("error").and_then(Value::as_str) {
        bail!("Ollama: {err}");
    }
    if let Some(piece) = v.pointer("/message/content").and_then(Value::as_str) {
        if !piece.is_empty() {
            text.push_str(piece);
            let visible = cut_think(text);
            if !visible.is_empty() {
                on_partial(&visible);
            }
        }
    }
    if v.get("done").and_then(Value::as_bool) == Some(true) {
        *timings = Timings::from_final(&v);
        return Ok(Some(true));
    }
    Ok(Some(false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn body_turns_thinking_off_and_pins_context() {
        let b = chat_body("qwen3.5:4b", "SYS", "USER", 1024, true, "30m", false);
        assert_eq!(b["model"], "qwen3.5:4b");
        assert_eq!(b["think"], false);
        assert_eq!(b["options"]["num_ctx"], NUM_CTX);
        assert_eq!(b["options"]["temperature"], 0);
        assert_eq!(b["options"]["num_predict"], 1024);
        assert_eq!(b["messages"][0]["content"], "SYS");
        assert_eq!(b["messages"][1]["role"], "user");
        assert_eq!(b["keep_alive"], "30m");
    }

    #[test]
    fn tags_match_with_implicit_latest() {
        assert!(tag_matches("qwen3.5:4b", "qwen3.5:4b"));
        assert!(tag_matches("llama3:latest", "llama3"));
        assert!(!tag_matches("qwen3.5:9b", "qwen3.5:4b"));
        assert!(!tag_matches("qwen3.5:4b-q8_0", "qwen3.5:4b"));
    }

    #[test]
    fn ndjson_stream_accumulates_and_reports_timings() {
        let seen = Mutex::new(Vec::<String>::new());
        let cb = |s: &str| seen.lock().unwrap().push(s.to_string());
        let (mut text, mut t) = (String::new(), Timings::default());
        let lines = [
            r#"{"message":{"role":"assistant","content":"see you "},"done":false}"#,
            r#"{"message":{"role":"assistant","content":"thursday"},"done":false}"#,
            r#"{"message":{"role":"assistant","content":""},"done":true,"prompt_eval_count":812,"prompt_eval_duration":95000000,"eval_count":6,"eval_duration":120000000,"load_duration":1000000}"#,
        ];
        let mut done = false;
        for l in lines {
            done = feed_ndjson(l.as_bytes(), &mut text, &mut t, &cb).unwrap() == Some(true);
        }
        assert!(done);
        assert_eq!(text, "see you thursday");
        assert_eq!(seen.lock().unwrap().last().unwrap(), "see you thursday");
        assert_eq!((t.prompt_tokens, t.prompt_ms as i64, t.generated_tokens), (812, 95, 6));
    }

    #[test]
    fn ndjson_error_line_fails() {
        let cb = |_: &str| {};
        let (mut text, mut t) = (String::new(), Timings::default());
        assert!(feed_ndjson(br#"{"error":"model 'x' not found"}"#, &mut text, &mut t, &cb).is_err());
    }
}
