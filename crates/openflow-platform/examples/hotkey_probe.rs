//! Installs the global hotkey tap (Cmd+; by default) and prints events for N seconds.
//! Usage: cargo run -p openflow-platform --example hotkey_probe [-- --seconds=N --key=41 --selftest]

#[cfg(target_os = "macos")]
fn main() {
    use std::sync::Arc;
    use std::time::Duration;

    use openflow_core::traits::Hotkey;
    use openflow_platform::macos::MacHotkey;

    tracing_subscriber::fmt().with_max_level(tracing::Level::INFO).init();
    let arg = |p: &str| std::env::args().find_map(|a| a.strip_prefix(p).and_then(|s| s.parse::<u64>().ok()));
    let seconds = arg("--seconds=").unwrap_or(2);
    let key = arg("--key=").unwrap_or(41) as u16;
    let hotkey = MacHotkey::default();
    hotkey
        .start(key, Arc::new(|e| println!("event: {e:?}")), Arc::new(|| true))
        .expect("start hotkey");
    std::thread::sleep(Duration::from_millis(500));
    println!("tap running: {}", hotkey.is_running());
    if std::env::args().any(|a| a == "--selftest") {
        // Synthetic Cmd+Shift+key down/up and Escape through the HID tap: the tap should report and swallow them.
        use objc2_core_graphics::{CGEvent, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation};
        let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState);
        let post = |code: u16, down: bool, flags: CGEventFlags| {
            let e = CGEvent::new_keyboard_event(source.as_deref(), code, down);
            CGEvent::set_flags(e.as_deref(), flags);
            CGEvent::post(CGEventTapLocation::HIDEventTap, e.as_deref());
        };
        let cmd_shift = CGEventFlags::MaskCommand | CGEventFlags::MaskShift;
        post(key, true, cmd_shift);
        post(key, false, cmd_shift);
        post(53, true, CGEventFlags::empty());
        post(53, false, CGEventFlags::empty());
    }
    std::thread::sleep(Duration::from_secs(seconds));
}

#[cfg(not(target_os = "macos"))]
fn main() {}
