//! OS integration behind the `openflow_core::traits` platform traits. Only this crate (and the sidecar) may call OS APIs.

pub mod detector;
pub mod hotkey_rules;
pub mod live_typer;
#[cfg(target_os = "macos")]
pub mod macos;
pub mod mic_watchdog;
pub mod text16;

use std::sync::Arc;

use openflow_core::traits::{Apps, AudioDevices, FieldReader, Hotkey, Keyboard, Mic, Sounds};

pub use detector::MeetingDetector;
pub use live_typer::LiveTyper;
pub use mic_watchdog::MicWatchdog;

/// Everything the orchestrator needs from the OS, except system audio and names (served by the sidecar on macOS).
#[derive(Clone)]
pub struct Platform {
    pub hotkey: Arc<dyn Hotkey>,
    pub field: Arc<dyn FieldReader>,
    pub keyboard: Arc<dyn Keyboard>,
    pub mic: Arc<dyn Mic>,
    pub devices: Arc<dyn AudioDevices>,
    pub apps: Arc<dyn Apps>,
    pub sounds: Arc<dyn Sounds>,
    /// A fresh, independent microphone (meetings record separately from dictation, as in the Swift app).
    pub new_mic: Arc<dyn Fn() -> Arc<dyn Mic> + Send + Sync>,
}

impl Platform {
    #[cfg(target_os = "macos")]
    pub fn native() -> Platform {
        macos::platform()
    }
}
