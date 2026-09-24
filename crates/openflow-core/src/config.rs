//! Everything user-configurable, stored in `<data dir>/config.json`.
//! Field names and defaults match the Swift app's `Config.swift` exactly, so both apps share one file.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize};

use crate::{default_tones, paths};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToneConfig {
    pub tone: String,
    pub alt: String,
}

impl ToneConfig {
    pub fn new(tone: &str, alt: &str) -> Self {
        Self { tone: tone.into(), alt: alt.into() }
    }
}

/// Missing keys fall back to defaults (`#[serde(default)]`), so hand-edited configs stay forgiving.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
    /// "v2" = Parakeet English-only, "v3" = 25 European languages.
    pub asr_version: String,
    pub llama_server_path: String,
    /// GGUF file name inside the models/ folder.
    pub model: String,
    pub llama_port: u16,
    /// "" = system default, "builtin" = the Mac's own mic, or a device UID.
    pub microphone: String,
    /// macOS virtual key code. 41 = ';'. Hotkey is Cmd + key; Cmd+Shift + key uses the alt tone.
    pub hotkey_key_code: u16,
    /// Seconds. A press shorter than this switches to toggle mode.
    pub tap_threshold: f64,
    pub default_tones: ToneConfig,
    /// Bundle identifier -> tones. Unlisted apps use `default_tones`.
    pub app_tones: BTreeMap<String, ToneConfig>,
    /// Tone name -> instruction for the polish model. Empty = verbatim (skip the model).
    pub tones: BTreeMap<String, String>,
    /// "Term" or "Term: sound-alike, sound-alike" per entry.
    pub dictionary: Vec<String>,
    pub learned_signals: Vec<String>,
    pub memory: String,
    pub keep_clips: bool,
    pub overlay: bool,
    pub sounds: bool,
    pub field_context: bool,
    pub live_text: bool,
    pub meeting_detect: bool,
    /// "auto" or "english".
    pub meeting_engine: String,
    /// "traditional" or "simplified".
    pub chinese_script: String,
    pub use_contacts: bool,
    pub window_at_launch: bool,
    pub cloud_enabled: bool,
    /// "off", "long" or "all". Legacy boolean values migrate to "all"/"off".
    #[serde(deserialize_with = "cloud_dictation")]
    pub cloud_dictation: String,
    pub cloud_meetings: bool,
    #[serde(rename = "liveTextExcluded")]
    pub live_text_excluded: Vec<String>,
    #[serde(rename = "cloudAPIKey")]
    pub cloud_api_key: String,
    #[serde(rename = "cloudASRModel")]
    pub cloud_asr_model: String,
    #[serde(rename = "cloudLLMModel")]
    pub cloud_llm_model: String,
    /// Speech server for meetings; "" = the bundled transcribe-server. (Was Homebrew's whisper-server.)
    pub whisper_server_path: String,
    /// GGUF in models/ that the speech server loads.
    pub whisper_model: String,
    /// "whisper" or "parakeet".
    pub meeting_far_engine: String,
    /// GGUF used for meeting notes when local; "" = the polish model.
    pub notes_model: String,
    /// Local LLM engine: "ollama" (default) or "llamacpp" (llama-server with a GGUF from models/).
    /// When Ollama is unavailable and a GGUF is installed, llama.cpp is used as the fallback.
    pub llm_backend: String,
    /// Ollama server, e.g. "http://127.0.0.1:11434".
    pub ollama_url: String,
    /// Ollama model tag for dictation cleanup.
    pub ollama_model: String,
    /// Ollama model tag for meeting notes; "" = the dictation model.
    pub ollama_notes_model: String,
    /// `ollama` binary used to start the server when it isn't running; "" = find it (Ollama.app or PATH).
    pub ollama_path: String,
}

fn cloud_dictation<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum V {
        B(bool),
        S(String),
    }
    Ok(match V::deserialize(d)? {
        V::B(true) => "all".into(),
        V::B(false) => "off".into(),
        V::S(s) => s,
    })
}

impl Default for Config {
    fn default() -> Self {
        let app_tones = [
            ("com.apple.MobileSMS", "supercasual", "professional"),
            ("net.whatsapp.WhatsApp", "supercasual", "professional"),
            ("com.tinyspeck.slackmacgap", "supercasual", "professional"),
            ("com.apple.mail", "professional", "casual"),
            ("com.google.Chrome", "professional", "casual"),
            ("com.apple.Safari", "professional", "casual"),
            ("com.apple.Terminal", "verbatim", "casual"),
            ("com.googlecode.iterm2", "verbatim", "casual"),
            ("com.microsoft.VSCode", "verbatim", "casual"),
        ]
        .into_iter()
        .map(|(b, t, a)| (b.to_string(), ToneConfig::new(t, a)))
        .collect();
        let tones = [
            ("supercasual", default_tones::SUPERCASUAL),
            ("casual", default_tones::CASUAL),
            ("professional", default_tones::PROFESSIONAL),
            ("verbatim", ""),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        Self {
            asr_version: "v2".into(),
            llama_server_path: "/opt/homebrew/bin/llama-server".into(),
            model: "Qwen3.5-4B-Q4_K_M.gguf".into(),
            llama_port: 8765,
            microphone: String::new(),
            hotkey_key_code: 41,
            tap_threshold: 0.5,
            default_tones: ToneConfig::new("supercasual", "professional"),
            app_tones,
            tones,
            dictionary: vec![
                "OpenFlow: open flow, open flo".into(),
                "no wait: no wake, no weight, know wait".into(),
            ],
            learned_signals: vec![],
            memory: String::new(),
            keep_clips: true,
            overlay: true,
            sounds: true,
            field_context: true,
            live_text: true,
            meeting_detect: true,
            meeting_engine: "auto".into(),
            chinese_script: "traditional".into(),
            use_contacts: false,
            window_at_launch: true,
            cloud_enabled: false,
            cloud_dictation: "long".into(),
            cloud_meetings: true,
            live_text_excluded: [
                "com.apple.Terminal",
                "com.googlecode.iterm2",
                "dev.warp.Warp-Stable",
                "net.kovidgoyal.kitty",
                "io.alacritty",
                "com.mitchellh.ghostty",
                "com.github.wez.wezterm",
                "co.zeit.hyper",
                "com.microsoft.VSCode",
            ]
            .map(String::from)
            .to_vec(),
            cloud_api_key: String::new(),
            cloud_asr_model: "whisper-large-v3-turbo".into(),
            cloud_llm_model: "qwen/qwen3.8-27b".into(),
            // "" = the transcribe-server bundled with the app.
            whisper_server_path: String::new(),
            whisper_model: crate::catalog::WHISPER_FILE.into(),
            meeting_far_engine: "whisper".into(),
            notes_model: "Qwen3.5-9B-Q4_K_M.gguf".into(),
            llm_backend: "ollama".into(),
            ollama_url: "http://127.0.0.1:11434".into(),
            ollama_model: "qwen3.5:4b".into(),
            ollama_notes_model: "qwen3.5:9b".into(),
            ollama_path: String::new(),
        }
    }
}

impl Config {
    pub fn model_path(&self) -> PathBuf {
        paths::models_dir().join(&self.model)
    }

    pub fn tones_for(&self, bundle_id: Option<&str>) -> &ToneConfig {
        bundle_id.and_then(|b| self.app_tones.get(b)).unwrap_or(&self.default_tones)
    }

    pub fn uses_ollama(&self) -> bool {
        self.llm_backend != "llamacpp"
    }

    pub fn traditional_chinese(&self) -> bool {
        self.chinese_script == "traditional"
    }

    /// Loads config.json, writing the defaults first if it doesn't exist yet.
    /// A file that fails to parse is copied to `config.json.bak` and defaults are used (Store.swift behaviour).
    pub fn load() -> anyhow::Result<Config> {
        crate::config_io::load()
    }

    /// Pretty-printed with sorted keys, like the Swift encoder.
    pub fn save(&self) -> anyhow::Result<()> {
        crate::config_io::save(self)
    }
}
