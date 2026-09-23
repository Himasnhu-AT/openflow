//! OWNER: team C (engines). Groq: transcribe (multipart WAV) + chat completions with model fallback (Cloud.swift).
//!
//! Optional cloud processing through Groq (OpenAI-compatible API): Whisper large v3 turbo for speech and a large
//! LLM for cleanup and notes. Off by default; when on, audio and text leave this machine. Live text while talking
//! always stays local. Every caller falls back to the local models on failure.

use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use jez_core::whisper_filter::{filter_whisper, WhisperSegment};
use jez_core::Config;
use serde_json::{json, Value};

use crate::prompts::{prefix_chars, trim_ws_nl};

pub const GROQ_BASE: &str = "https://api.groq.com/openai/v1";

/// Preview models come and go on Groq; if the configured one is missing or unavailable, the production gpt-oss
/// models take over for this call (the switch is logged, the setting is left as is).
pub const FALLBACK_MODELS: [&str; 2] = ["openai/gpt-oss-120b", "openai/gpt-oss-20b"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Dictation,
    Meetings,
}

#[derive(Debug, Clone)]
pub struct Cloud {
    pub api_key: String,
    pub asr_model: String,
    pub llm_model: String,
    base: String,
    http: reqwest::Client,
}

/// Swift `.trimmingCharacters(in: .whitespaces)` (no newlines).
fn trim_spaces(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() && !matches!(c, '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}'))
}

/// The routing rules of `Cloud.init?`, without the environment. `long` = the current dictation counts as long
/// (over 15 s or a correction signal in the live text), for the "long" dictation mode.
pub fn routes(config: &Config, scope: Scope, long: bool) -> bool {
    if !config.cloud_enabled || trim_spaces(&config.cloud_api_key).is_empty() {
        return false;
    }
    match scope {
        Scope::Meetings => config.cloud_meetings,
        Scope::Dictation => config.cloud_dictation == "all" || (config.cloud_dictation == "long" && long),
    }
}

/// Multipart form body exactly as Cloud.swift / WhisperServer.swift write it: text fields, then `file` =
/// `audio.wav` (audio/wav), then the closing boundary.
pub(crate) fn multipart_body(boundary: &str, fields: &[(&str, &str)], wav: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(wav.len() + 512);
    for (name, value) in fields {
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes(),
        );
    }
    body.extend_from_slice(
        format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\n")
            .as_bytes(),
    );
    body.extend_from_slice(wav);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

/// `jezflow-<UUID>` with the uppercase UUID Foundation prints.
pub(crate) fn new_boundary() -> String {
    format!("jezflow-{}", uuid::Uuid::new_v4().to_string().to_uppercase())
}

/// `segments` of a verbose_json reply; entries that don't parse fall back to the Swift defaults field by field.
pub(crate) fn segments_of(json: &Value) -> Vec<WhisperSegment> {
    json.get("segments")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter(|v| v.is_object())
                .map(|v| WhisperSegment {
                    text: v.get("text").and_then(Value::as_str).unwrap_or_default().to_string(),
                    no_speech_prob: v.get("no_speech_prob").and_then(Value::as_f64).unwrap_or(0.0),
                    avg_logprob: v.get("avg_logprob").and_then(Value::as_f64).unwrap_or(0.0),
                    compression_ratio: v.get("compression_ratio").and_then(Value::as_f64).unwrap_or(1.0),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Everything after `</think>` (if present), trimmed.
pub(crate) fn cut_think(content: &str) -> String {
    let c = content.find("</think>").map_or(content, |i| &content[i + "</think>".len()..]);
    trim_ws_nl(c).to_string()
}

/// Error substrings that move `complete` on to the next model instead of failing.
pub fn is_model_unavailable(message: &str) -> bool {
    [
        "does not exist",
        "not have access",
        "decommissioned",
        "model_not_found",
        "too large",
        "rate_limit",
        "Rate limit",
        "429",
    ]
    .iter()
    .any(|s| message.contains(s))
}

/// Request body for one chat completion (`complete(model:)`).
pub fn chat_body(model: &str, system: &str, user: &str, max_tokens: usize) -> Value {
    let mut body = json!({
        "model": model,
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
        "temperature": 0,
        // Groq's on-demand tier caps output at 1,000 tokens per request.
        "max_tokens": max_tokens.min(900),
    });
    if model.starts_with("qwen") {
        body["reasoning_effort"] = json!("none"); // answer directly, no thinking
    } else if model.contains("gpt-oss") {
        body["reasoning_effort"] = json!("low");
        body["reasoning_format"] = json!("hidden");
    }
    body
}

impl Cloud {
    /// `Cloud(config:for:long:)`: None unless the cloud is on, has a key and applies to this scope.
    /// `JEZ_CLOUD_LLM` overrides the LLM model.
    pub fn new(config: &Config, scope: Scope, long: bool) -> Option<Cloud> {
        Self::with_env(config, scope, long, std::env::var("JEZ_CLOUD_LLM").ok())
    }

    /// `new` with the `JEZ_CLOUD_LLM` value passed in (tests).
    pub fn with_env(config: &Config, scope: Scope, long: bool, llm_override: Option<String>) -> Option<Cloud> {
        if !routes(config, scope, long) {
            return None;
        }
        Some(Cloud {
            api_key: trim_spaces(&config.cloud_api_key).to_string(),
            asr_model: config.cloud_asr_model.clone(),
            llm_model: llm_override.unwrap_or_else(|| config.cloud_llm_model.clone()),
            base: GROQ_BASE.to_string(),
            http: reqwest::Client::new(),
        })
    }

    /// Points the client at another OpenAI-compatible base URL (tests use a local mock server).
    pub fn with_base_url(mut self, base: impl Into<String>) -> Self {
        self.base = base.into().trim_end_matches('/').to_string();
        self
    }

    /// 16 kHz mono samples -> text, through `filter_whisper`. No `prompt` field is sent: Whisper echoes prompt
    /// terms on silence and at clip ends ("Acme Hub, Acme Hub, Acme Hub"); names are fixed afterwards by the
    /// dictionary rules instead.
    pub async fn transcribe(&self, samples: &[f32], language: Option<&str>) -> Result<String> {
        let boundary = new_boundary();
        let mut fields = vec![("model", self.asr_model.as_str()), ("response_format", "verbose_json"), ("temperature", "0")];
        if let Some(l) = language {
            fields.push(("language", l));
        }
        let body = multipart_body(&boundary, &fields, &jez_core::wav::pcm16_bytes(samples));
        let t0 = Instant::now();
        let resp = self
            .http
            .post(format!("{}/audio/transcriptions", self.base))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", format!("multipart/form-data; boundary={boundary}"))
            .body(body)
            .timeout(Duration::from_secs(20))
            .send()
            .await?;
        let status = resp.status().as_u16();
        let data = resp.bytes().await?;
        let json: Option<Value> = serde_json::from_slice(&data).ok();
        let text = json.as_ref().and_then(|j| j.get("text")).and_then(Value::as_str);
        let (Some(json), Some(text), 200) = (json.as_ref(), text, status) else {
            bail!("Cloud speech failed: {}", prefix_chars(&String::from_utf8_lossy(&data), 200));
        };
        tracing::info!(target: "cloud", "Cloud ASR {} ms for {} ms audio", t0.elapsed().as_millis(), samples.len() / 16);
        Ok(filter_whisper(text, &segments_of(json), samples.len() as f64 / 16_000.0))
    }

    /// Chat completion, trying the configured model and then the fallbacks when a model is unavailable.
    pub async fn complete(&self, system: &str, user: &str, max_tokens: usize) -> Result<String> {
        let mut models = vec![self.llm_model.clone()];
        models.extend(FALLBACK_MODELS.iter().filter(|m| **m != self.llm_model).map(|m| m.to_string()));
        let mut last_error = None;
        for model in models {
            match self.complete_with(&model, system, user, max_tokens).await {
                Ok(t) => return Ok(t),
                Err(e) => {
                    if !is_model_unavailable(&e.to_string()) {
                        return Err(e);
                    }
                    tracing::error!(target: "cloud", "Cloud model {model} unavailable, trying the next one");
                    last_error = Some(e);
                }
            }
        }
        Err(last_error.unwrap_or_else(|| anyhow!("No cloud model available")))
    }

    async fn complete_with(&self, model: &str, system: &str, user: &str, max_tokens: usize) -> Result<String> {
        let t0 = Instant::now();
        let resp = self
            .http
            .post(format!("{}/chat/completions", self.base))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .header("User-Agent", "JezFlow/0.1")
            .body(serde_json::to_vec(&chat_body(model, system, user, max_tokens))?)
            .timeout(Duration::from_secs(30))
            .send()
            .await?;
        let status = resp.status().as_u16();
        let data = resp.bytes().await?;
        let content = serde_json::from_slice::<Value>(&data)
            .ok()
            .and_then(|j| j.pointer("/choices/0/message/content").and_then(Value::as_str).map(str::to_string));
        let (Some(content), 200) = (content, status) else {
            bail!("Cloud model failed: {}", prefix_chars(&String::from_utf8_lossy(&data), 200));
        };
        tracing::info!(target: "cloud", "Cloud LLM {} ms", t0.elapsed().as_millis());
        Ok(cut_think(&content))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(enabled: bool, key: &str, dictation: &str, meetings: bool) -> Config {
        Config {
            cloud_enabled: enabled,
            cloud_api_key: key.into(),
            cloud_dictation: dictation.into(),
            cloud_meetings: meetings,
            ..Config::default()
        }
    }

    #[test]
    fn routing_matrix() {
        // (enabled, key, dictation mode, meetings, scope, long) -> routes
        let cases = [
            (false, "k", "all", true, Scope::Dictation, true, false),
            (true, "", "all", true, Scope::Dictation, true, false),
            (true, "  \t", "all", true, Scope::Meetings, false, false),
            (true, "k", "all", false, Scope::Dictation, false, true),
            (true, "k", "long", false, Scope::Dictation, false, false),
            (true, "k", "long", false, Scope::Dictation, true, true),
            (true, "k", "off", true, Scope::Dictation, true, false),
            (true, "k", "off", true, Scope::Meetings, false, true),
            (true, "k", "all", false, Scope::Meetings, true, false),
            (true, "k", "ALL", true, Scope::Dictation, true, false),
        ];
        for (en, key, dict, meet, scope, long, want) in cases {
            let c = cfg(en, key, dict, meet);
            assert_eq!(routes(&c, scope, long), want, "{en} {key:?} {dict} {meet} {scope:?} {long}");
            assert_eq!(Cloud::with_env(&c, scope, long, None).is_some(), want);
        }
    }

    #[test]
    fn key_trimmed_and_env_override() {
        let c = cfg(true, "  gsk_abc \t", "all", true);
        let cloud = Cloud::with_env(&c, Scope::Dictation, false, None).unwrap();
        assert_eq!(cloud.api_key, "gsk_abc");
        assert_eq!(cloud.llm_model, c.cloud_llm_model);
        let cloud = Cloud::with_env(&c, Scope::Dictation, false, Some("x/y".into())).unwrap();
        assert_eq!(cloud.llm_model, "x/y");
    }

    #[test]
    fn body_reasoning_flags() {
        let b = chat_body("qwen/qwen3-32b", "s", "u", 1024);
        assert_eq!(b["max_tokens"], 900);
        assert_eq!(b["reasoning_effort"], "none");
        assert!(b.get("reasoning_format").is_none());
        let b = chat_body("openai/gpt-oss-20b", "s", "u", 24);
        assert_eq!(b["max_tokens"], 24);
        assert_eq!(b["reasoning_effort"], "low");
        assert_eq!(b["reasoning_format"], "hidden");
        let b = chat_body("llama-3.3-70b", "s", "u", 24);
        assert!(b.get("reasoning_effort").is_none());
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["messages"][1]["content"], "u");
        assert_eq!(b["temperature"], 0);
    }

    #[test]
    fn unavailable_errors() {
        for m in ["The model `x` does not exist", "you do not have access", "429 Too Many", "Rate limit reached", "rate_limit_exceeded",
                  "model_not_found", "has been decommissioned", "Request too large"] {
            assert!(is_model_unavailable(m), "{m}");
        }
        assert!(!is_model_unavailable("invalid_api_key"));
    }

    #[test]
    fn multipart_shape() {
        let b = multipart_body("jezflow-X", &[("model", "m"), ("temperature", "0")], b"WAV");
        let want = "--jezflow-X\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nm\r\n\
--jezflow-X\r\nContent-Disposition: form-data; name=\"temperature\"\r\n\r\n0\r\n\
--jezflow-X\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\nWAV\r\n--jezflow-X--\r\n";
        assert_eq!(String::from_utf8(b).unwrap(), want);
        let bd = new_boundary();
        assert!(bd.starts_with("jezflow-") && bd.len() == 8 + 36 && bd == bd.to_uppercase().replace("JEZFLOW", "jezflow"));
    }

    #[test]
    fn think_cut() {
        assert_eq!(cut_think("<think>x</think>\n Hi "), "Hi");
        assert_eq!(cut_think(" plain "), "plain");
    }

    #[test]
    fn segments_defaults() {
        let j = json!({"segments": [{"text": "a", "avg_logprob": -1.5}, {}, 3]});
        let s = segments_of(&j);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].avg_logprob, -1.5);
        assert_eq!(s[1].compression_ratio, 1.0);
        assert!(segments_of(&json!({})).is_empty());
    }
}
