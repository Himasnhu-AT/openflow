//! Global hotkey (HotkeyMonitor.swift): a CGEventTap on keyDown/keyUp at the head of the session tap, running
//! on its own thread and CFRunLoop. Needs the Accessibility permission; retries every 3 s until it's granted.
//! The swallow rules live in `hotkey_rules::decide`.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use jez_core::traits::{Hotkey, HotkeySink, RecordingProbe};
use objc2_core_foundation::{kCFRunLoopCommonModes, CFMachPort, CFRunLoop};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventTapProxy, CGEventType,
};

use crate::hotkey_rules::{decide, Action, EventKind, KeyEvent, Modifiers};

struct TapContext {
    key_code: u16,
    sink: HotkeySink,
    is_recording: RecordingProbe,
    port: AtomicPtr<CFMachPort>,
}

unsafe extern "C-unwind" fn callback(
    _proxy: CGEventTapProxy,
    event_type: CGEventType,
    event: NonNull<CGEvent>,
    user_info: *mut c_void,
) -> *mut CGEvent {
    let ctx = unsafe { &*(user_info as *const TapContext) };
    let ev = unsafe { event.as_ref() };
    let kind = match event_type {
        CGEventType::KeyDown => EventKind::KeyDown,
        CGEventType::KeyUp => EventKind::KeyUp,
        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => EventKind::TapDisabled,
        _ => EventKind::Other,
    };
    let flags = CGEvent::flags(Some(ev));
    let key = KeyEvent {
        kind,
        code: CGEvent::integer_value_field(Some(ev), CGEventField::KeyboardEventKeycode) as u16,
        modifiers: Modifiers {
            command: flags.contains(CGEventFlags::MaskCommand),
            control: flags.contains(CGEventFlags::MaskControl),
            option: flags.contains(CGEventFlags::MaskAlternate),
            shift: flags.contains(CGEventFlags::MaskShift),
        },
        autorepeat: CGEvent::integer_value_field(Some(ev), CGEventField::KeyboardEventAutorepeat) != 0,
    };
    match decide(&key, ctx.key_code, || (ctx.is_recording)()) {
        Action::Pass => event.as_ptr(),
        Action::ReenableAndPass => {
            // macOS disables the tap if we're ever too slow; re-enable and move on.
            let port = ctx.port.load(Ordering::SeqCst);
            if let Some(port) = unsafe { port.as_ref() } {
                CGEvent::tap_enable(port, true);
            }
            event.as_ptr()
        }
        Action::Swallow => std::ptr::null_mut(),
        Action::Emit(e) => {
            (ctx.sink)(e);
            std::ptr::null_mut()
        }
    }
}

fn run_tap(ctx: &'static TapContext, running: Arc<AtomicBool>) {
    let mask: u64 = (1 << CGEventType::KeyDown.0) | (1 << CGEventType::KeyUp.0);
    let tap = loop {
        let tap = unsafe {
            CGEvent::tap_create(
                CGEventTapLocation::SessionEventTap,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::Default,
                mask,
                Some(callback),
                ctx as *const TapContext as *mut c_void,
            )
        };
        match tap {
            Some(t) => break t,
            None => {
                tracing::warn!(target: "hotkey", "Event tap unavailable (Accessibility not granted yet); retrying in 3s");
                thread::sleep(Duration::from_secs(3));
            }
        }
    };
    ctx.port.store(&*tap as *const CFMachPort as *mut CFMachPort, Ordering::SeqCst);
    let Some(source) = CFMachPort::new_run_loop_source(None, Some(&tap), 0) else {
        tracing::error!(target: "hotkey", "Event tap run loop source unavailable");
        return;
    };
    let Some(run_loop) = CFRunLoop::current() else { return };
    run_loop.add_source(Some(&source), unsafe { kCFRunLoopCommonModes });
    CGEvent::tap_enable(&tap, true);
    running.store(true, Ordering::SeqCst);
    tracing::info!(target: "hotkey", "Event tap installed");
    CFRunLoop::run();
    running.store(false, Ordering::SeqCst);
    drop(tap);
}

#[derive(Default)]
pub struct MacHotkey {
    started: AtomicBool,
    running: Arc<AtomicBool>,
}

impl Hotkey for MacHotkey {
    /// `sink` and `is_recording` are called on the tap thread, inside the event callback: keep them quick and
    /// non-blocking (macOS disables a slow tap).
    fn start(&self, key_code: u16, sink: HotkeySink, is_recording: RecordingProbe) -> anyhow::Result<()> {
        if self.started.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        // Lives for the rest of the process, like the tap itself.
        let ctx: &'static TapContext =
            Box::leak(Box::new(TapContext { key_code, sink, is_recording, port: AtomicPtr::new(std::ptr::null_mut()) }));
        let running = Arc::clone(&self.running);
        thread::Builder::new().name("jezflow-hotkey".into()).spawn(move || run_tap(ctx, running))?;
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}
