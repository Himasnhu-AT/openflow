//! Contracts between crates. Pure definitions only: implementations live in `jez-engines` (engines, sidecar)
//! and `jez-platform` (OS integration). Every OS port implements the platform traits; nothing above them
//! may use OS APIs directly.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::field_context::FieldContext;
use crate::model::{AppInfo, InputDevice};

pub type Result<T> = anyhow::Result<T>;

/// Correction examples shown to the polish model, per tone: (heard, wanted), oldest first.
pub type Examples = BTreeMap<String, Vec<(String, String)>>;

// ---------------------------------------------------------------------------------------------- engines

/// Cleans a transcript with an LLM (local llama-server or cloud). Implemented by `jez_engines::Polisher`.
#[async_trait]
pub trait PolishBackend: Send + Sync {
    #[allow(clippy::too_many_arguments)]
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
    ) -> Result<String>;
}

/// Speech recognition. macOS: FluidAudio in the Swift sidecar. Windows/Linux later: sherpa-onnx.
/// Audio is always 16 kHz mono f32.
#[async_trait]
pub trait AsrEngine: Send + Sync {
    /// Load (downloading on first use) Parakeet "v2" or "v3".
    async fn load(&self, version: &str) -> Result<()>;
    fn is_ready(&self) -> bool;
    /// One decode. `rescore` applies dictionary/vocabulary boosting (CTC rescoring on macOS).
    async fn transcribe(&self, samples: &[f32], rescore: bool) -> Result<String>;
    /// Terms to boost (already merged and filtered to >= 4 chars by the caller). Unchanged lists are a no-op.
    async fn set_vocabulary(&self, terms: &[String]) -> Result<()>;
    /// Multilingual fallback for meetings (SenseVoice). Loads lazily.
    async fn transcribe_multilingual(&self, samples: &[f32]) -> Result<String>;
}

// ---------------------------------------------------------------------------------------------- platform

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HotkeyEvent {
    /// Cmd+key down (not autorepeat). `alt` = Shift held.
    Press { alt: bool },
    Release,
    /// Escape while recording (swallowed).
    Escape,
}

pub type HotkeySink = Arc<dyn Fn(HotkeyEvent) + Send + Sync>;
pub type RecordingProbe = Arc<dyn Fn() -> bool + Send + Sync>;

pub trait Hotkey: Send + Sync {
    /// Installs the global hook; retries until permission is granted. Events are delivered on any thread.
    fn start(&self, key_code: u16, sink: HotkeySink, is_recording: RecordingProbe) -> Result<()>;
    fn is_running(&self) -> bool;
}

/// Reads and edits the focused text field (Accessibility on macOS, UI Automation on Windows).
pub trait FieldReader: Send + Sync {
    fn capture(&self) -> FieldContext;
    /// Replace `length` UTF-16 units at `location` (which should currently hold `expecting`) with `text`,
    /// verified by reading back. Returns false when the field can't be edited exactly.
    fn replace(&self, location: usize, length: usize, expecting: &str, text: &str) -> bool;
    fn frontmost_app(&self) -> Option<AppInfo>;
    fn is_trusted(&self, prompt: bool) -> bool;
}

/// Low-level keyboard + clipboard used by the (cross-platform) LiveTyper.
pub trait Keyboard: Send + Sync {
    fn paste_shortcut(&self);
    fn backspace(&self, count: usize);
    fn clipboard_get(&self) -> Option<String>;
    fn clipboard_set(&self, text: &str);
}

pub type ChunkSink = Arc<dyn Fn(&[f32]) + Send + Sync>;
pub type LevelSink = Arc<dyn Fn(f32) + Send + Sync>;

/// Microphone at 16 kHz mono f32. Stays warm for 20 s after stop.
pub trait Mic: Send + Sync {
    fn start(&self, preferred_device: &str) -> Result<()>;
    /// Copy of everything captured since `start`.
    fn snapshot(&self) -> Vec<f32>;
    fn stop(&self) -> Vec<f32>;
    fn shutdown(&self);
    fn is_running(&self) -> bool;
    fn set_on_level(&self, sink: Option<LevelSink>);
    /// Every raw 16 kHz chunk, also while not capturing (meetings use this).
    fn set_on_chunk(&self, sink: Option<ChunkSink>);
    fn current_device_uid(&self) -> Option<String>;
}

pub trait AudioDevices: Send + Sync {
    fn inputs(&self) -> Vec<InputDevice>;
    /// "" = default (avoiding the built-in mic while the lid is closed), "builtin", or a UID.
    fn resolve(&self, preference: &str) -> Option<InputDevice>;
    fn lid_closed(&self) -> bool;
    /// Default input device is in use by any process.
    fn microphone_in_use(&self) -> bool;
}

/// Other side of calls: system/app audio at 16 kHz mono (Core Audio process tap on macOS).
pub trait SystemAudio: Send + Sync {
    /// `app_bundle` "" = all system audio.
    fn start(&self, app_bundle: &str, sink: ChunkSink) -> Result<()>;
    fn stop(&self);
}

pub trait Apps: Send + Sync {
    fn running(&self) -> Vec<AppInfo>;
    fn info(&self, bundle_id: &str) -> Option<AppInfo>;
    /// Window titles of a running app (browser call-tab detection).
    fn window_titles(&self, bundle_id: &str) -> Vec<String>;
}

#[async_trait]
pub trait NameSource: Send + Sync {
    /// Contacts + next 48 h of calendar names, de-duplicated. Empty when not permitted.
    async fn names(&self) -> Vec<String>;
}

pub trait Sounds: Send + Sync {
    fn start_cue(&self);
    fn stop_cue(&self);
}
