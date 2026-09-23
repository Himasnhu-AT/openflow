//! OS integration behind the `jez_core::traits` platform traits. Only this crate (and the sidecar) may call OS APIs.

pub mod detector;
pub mod live_typer;
#[cfg(target_os = "macos")]
pub mod macos;

use std::sync::Arc;

use jez_core::traits::{Apps, AudioDevices, FieldReader, Hotkey, Keyboard, Mic, Sounds};

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
}

impl Platform {
    #[cfg(target_os = "macos")]
    pub fn native() -> Platform {
        macos::platform()
    }
}
