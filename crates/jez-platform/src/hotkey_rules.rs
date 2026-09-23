//! What the global hotkey hook does with each key event (HotkeyMonitor.swift `handle`), independent of the OS
//! hook so it can be unit-tested and reused by the Windows/Linux ports.

use jez_core::traits::HotkeyEvent;

/// Escape's virtual key code on macOS.
pub const ESCAPE: u16 = 53;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    KeyDown,
    KeyUp,
    /// The OS disabled the hook (timeout or user input): re-enable it.
    TapDisabled,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub command: bool,
    pub control: bool,
    pub option: bool,
    pub shift: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub kind: EventKind,
    pub code: u16,
    pub modifiers: Modifiers,
    pub autorepeat: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Let the event through untouched.
    Pass,
    /// Re-enable the hook, then let the event through.
    ReenableAndPass,
    /// Drop the event; nothing to report (autorepeat).
    Swallow,
    /// Drop the event and report it.
    Emit(HotkeyEvent),
}

impl Action {
    /// True when the event must not reach the frontmost app.
    pub fn swallows(self) -> bool {
        matches!(self, Action::Swallow | Action::Emit(_))
    }
}

/// Exact HotkeyMonitor rules:
/// - Escape keyDown while recording is swallowed and reported (`is_recording` is only consulted then).
/// - Other keys pass.
/// - keyDown of the hotkey needs Cmd and neither Ctrl nor Option; autorepeat is swallowed silently; Shift = alt.
/// - keyUp of the hotkey is always swallowed and reported, even when Cmd was released first (so a plain keyUp of
///   the key is swallowed when idle too, as in the Swift app).
pub fn decide(event: &KeyEvent, key_code: u16, is_recording: impl FnOnce() -> bool) -> Action {
    if event.kind == EventKind::TapDisabled {
        return Action::ReenableAndPass;
    }
    if event.code == ESCAPE && event.kind == EventKind::KeyDown && is_recording() {
        return Action::Emit(HotkeyEvent::Escape);
    }
    if event.code != key_code {
        return Action::Pass;
    }
    match event.kind {
        EventKind::KeyDown => {
            let m = event.modifiers;
            if !m.command || m.control || m.option {
                return Action::Pass;
            }
            if event.autorepeat {
                return Action::Swallow;
            }
            Action::Emit(HotkeyEvent::Press { alt: m.shift })
        }
        EventKind::KeyUp => Action::Emit(HotkeyEvent::Release),
        _ => Action::Pass,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: u16 = 41; // ';'

    fn ev(kind: EventKind, code: u16, m: Modifiers, autorepeat: bool) -> KeyEvent {
        KeyEvent { kind, code, modifiers: m, autorepeat }
    }
    fn cmd() -> Modifiers {
        Modifiers { command: true, ..Default::default() }
    }

    #[test]
    fn press_and_release() {
        let down = ev(EventKind::KeyDown, KEY, cmd(), false);
        assert_eq!(decide(&down, KEY, || false), Action::Emit(HotkeyEvent::Press { alt: false }));
        let alt = ev(EventKind::KeyDown, KEY, Modifiers { shift: true, ..cmd() }, false);
        assert_eq!(decide(&alt, KEY, || false), Action::Emit(HotkeyEvent::Press { alt: true }));
        // Release counts without Cmd too.
        let up = ev(EventKind::KeyUp, KEY, Modifiers::default(), false);
        assert_eq!(decide(&up, KEY, || false), Action::Emit(HotkeyEvent::Release));
    }

    #[test]
    fn modifiers_required() {
        let plain = ev(EventKind::KeyDown, KEY, Modifiers::default(), false);
        assert_eq!(decide(&plain, KEY, || false), Action::Pass);
        let ctrl = ev(EventKind::KeyDown, KEY, Modifiers { control: true, ..cmd() }, false);
        assert_eq!(decide(&ctrl, KEY, || false), Action::Pass);
        let opt = ev(EventKind::KeyDown, KEY, Modifiers { option: true, ..cmd() }, false);
        assert_eq!(decide(&opt, KEY, || false), Action::Pass);
    }

    #[test]
    fn autorepeat_swallowed() {
        let rep = ev(EventKind::KeyDown, KEY, cmd(), true);
        assert_eq!(decide(&rep, KEY, || false), Action::Swallow);
        assert!(Action::Swallow.swallows());
        // Autorepeat without Cmd passes (modifier check comes first).
        let rep_plain = ev(EventKind::KeyDown, KEY, Modifiers::default(), true);
        assert_eq!(decide(&rep_plain, KEY, || false), Action::Pass);
    }

    #[test]
    fn escape_only_while_recording() {
        let esc = ev(EventKind::KeyDown, ESCAPE, Modifiers::default(), false);
        assert_eq!(decide(&esc, KEY, || true), Action::Emit(HotkeyEvent::Escape));
        assert_eq!(decide(&esc, KEY, || false), Action::Pass);
        let esc_up = ev(EventKind::KeyUp, ESCAPE, Modifiers::default(), false);
        assert_eq!(decide(&esc_up, KEY, || panic!("not consulted")), Action::Pass);
    }

    #[test]
    fn recording_probe_only_for_escape() {
        let other = ev(EventKind::KeyDown, 0, cmd(), false);
        assert_eq!(decide(&other, KEY, || panic!("not consulted")), Action::Pass);
        let down = ev(EventKind::KeyDown, KEY, cmd(), false);
        assert!(decide(&down, KEY, || panic!("not consulted")).swallows());
    }

    #[test]
    fn tap_disabled() {
        let d = ev(EventKind::TapDisabled, 0, Modifiers::default(), false);
        assert_eq!(decide(&d, KEY, || false), Action::ReenableAndPass);
        assert!(!Action::ReenableAndPass.swallows());
    }
}
