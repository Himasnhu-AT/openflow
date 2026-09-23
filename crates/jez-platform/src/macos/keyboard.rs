//! Synthetic keystrokes (CGEvent → HID tap, combined-session source) and the plain-text clipboard (NSPasteboard).

use jez_core::traits::{Keyboard, Sounds};
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString, NSSound};
use objc2_core_graphics::{CGEvent, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation};
use objc2_foundation::NSString;

const KEY_V: u16 = 9;
const KEY_BACKSPACE: u16 = 51;

fn key(code: u16, flags: CGEventFlags) {
    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState);
    for down in [true, false] {
        let event = CGEvent::new_keyboard_event(source.as_deref(), code, down);
        CGEvent::set_flags(event.as_deref(), flags);
        CGEvent::post(CGEventTapLocation::HIDEventTap, event.as_deref());
    }
}

pub struct MacKeyboard;

impl Keyboard for MacKeyboard {
    fn paste_shortcut(&self) {
        key(KEY_V, CGEventFlags::MaskCommand);
    }

    fn backspace(&self, count: usize) {
        for _ in 0..count {
            key(KEY_BACKSPACE, CGEventFlags::empty());
        }
    }

    fn clipboard_get(&self) -> Option<String> {
        let pb = NSPasteboard::generalPasteboard();
        pb.stringForType(unsafe { NSPasteboardTypeString }).map(|s| s.to_string())
    }

    fn clipboard_set(&self, text: &str) {
        let pb = NSPasteboard::generalPasteboard();
        pb.clearContents();
        pb.setString_forType(&NSString::from_str(text), unsafe { NSPasteboardTypeString });
    }
}

/// Overlay.swift `Cue`: system sounds at a quarter volume.
pub struct MacSounds;

fn play(name: &str) {
    if let Some(sound) = NSSound::soundNamed(&NSString::from_str(name)) {
        sound.setVolume(0.25);
        sound.play();
    }
}

impl Sounds for MacSounds {
    fn start_cue(&self) {
        play("Tink");
    }

    fn stop_cue(&self) {
        play("Pop");
    }
}
