//! OWNER: team E (platform). macOS implementations: CGEventTap hotkey, AX field reader, CGEvent keyboard +
//! NSPasteboard, mic (cpal/CoreAudio -> 16 kHz), Core Audio devices + lid state, NSWorkspace apps, NSSound cues.

pub mod apps;
pub mod ax;
pub mod devices;
pub mod hotkey;
pub mod keyboard;
pub mod mic;

use std::sync::Arc;

pub use apps::MacApps;
pub use ax::AxFieldReader;
pub use devices::MacAudioDevices;
pub use hotkey::MacHotkey;
pub use keyboard::{MacKeyboard, MacSounds};
pub use mic::MacMic;

pub fn platform() -> crate::Platform {
    crate::Platform {
        hotkey: Arc::new(MacHotkey::default()),
        field: Arc::new(AxFieldReader),
        keyboard: Arc::new(MacKeyboard),
        mic: Arc::new(MacMic::new()),
        devices: Arc::new(MacAudioDevices),
        apps: Arc::new(MacApps::default()),
        sounds: Arc::new(MacSounds),
    }
}
