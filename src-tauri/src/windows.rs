//! Main window and the floating pill (overlay window).

use tauri::{AppHandle, Emitter, Manager};

use crate::state::{OverlayMode, OverlayState};

/// Shows the main window, optionally on a page ("Home", "Models", ...), and brings the app forward.
pub fn open_main(app: &AppHandle, page: Option<&str>) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        if let Some(p) = page {
            let _ = app.emit_to("main", "navigate", p.to_string());
        }
    }
}

/// Pill size for a mode, as in Overlay.swift.
fn pill_size(state: &OverlayState) -> (f64, f64) {
    match state.mode {
        OverlayMode::MeetingOffer { .. } | OverlayMode::MeetingLive { .. } => (360.0, 52.0),
        OverlayMode::Listening if !state.live_text.is_empty() => (520.0, 74.0),
        _ => (200.0, 52.0),
    }
}

pub fn apply_overlay(app: &AppHandle, state: &OverlayState) {
    let Some(w) = app.get_webview_window("overlay") else { return };
    let hidden = state.mode == OverlayMode::Hidden;
    let interactive = matches!(state.mode, OverlayMode::MeetingOffer { .. } | OverlayMode::MeetingLive { .. });
    let (width, height) = pill_size(state);
    let _ = w.set_ignore_cursor_events(!interactive);
    #[cfg(target_os = "macos")]
    {
        let w2 = w.clone();
        let _ = w.run_on_main_thread(move || macos::place(&w2, hidden, width, height));
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = w.set_size(tauri::LogicalSize::new(width, height));
        if hidden {
            let _ = w.hide();
        } else {
            let _ = w.show();
        }
    }
}

/// One-time panel setup: status-bar level, all spaces, never activates or takes focus on show.
pub fn setup_overlay(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    if let Some(w) = app.get_webview_window("overlay") {
        let w2 = w.clone();
        let _ = w.run_on_main_thread(move || macos::configure(&w2));
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{msg_send, MainThreadMarker};
    use objc2_app_kit::{NSColor, NSEvent, NSScreen, NSWindow, NSWindowCollectionBehavior};
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    use tauri::WebviewWindow;

    /// NSStatusWindowLevel.
    const STATUS_BAR_LEVEL: isize = 25;

    fn ns_window(w: &WebviewWindow) -> Option<Retained<NSWindow>> {
        let ptr = w.ns_window().ok()? as *mut NSWindow;
        // SAFETY: Tauri hands out the live NSWindow of this webview window; we only use it on the main thread.
        unsafe { Retained::retain(ptr) }
    }

    pub fn configure(w: &WebviewWindow) {
        let Some(win) = ns_window(w) else { return };
        win.setLevel(STATUS_BAR_LEVEL);
        win.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Stationary,
        );
        win.setHidesOnDeactivate(false);
        win.setHasShadow(true);
        // No transparent webview (that needs Tauri's private macOS API, which the App Store rejects): the page is
        // opaque and the window is clipped to a capsule instead. Everything here is public AppKit API.
        win.setOpaque(false);
        win.setBackgroundColor(Some(&NSColor::clearColor()));
    }

    /// Rounds the window's content to a capsule of this height (the pill fills its window).
    fn clip_to_capsule(win: &NSWindow, height: f64) {
        let Some(view) = win.contentView() else { return };
        view.setWantsLayer(true);
        // SAFETY: `layer` is a public NSView property; after setWantsLayer(true) it is a live CALayer, and
        // setCornerRadius: / setMasksToBounds: are public CALayer setters. Main thread only (caller).
        unsafe {
            let layer: *mut AnyObject = msg_send![&*view, layer];
            if let Some(layer) = layer.as_ref() {
                let _: () = msg_send![layer, setCornerRadius: height / 2.0];
                let _: () = msg_send![layer, setMasksToBounds: true];
            }
        }
        win.invalidateShadow();
    }

    /// Centered horizontally on the screen under the mouse, 28 pt above the bottom of its visible frame.
    /// `orderFrontRegardless` shows it without activating OpenFlow, so the field being dictated into keeps focus.
    pub fn place(w: &WebviewWindow, hidden: bool, width: f64, height: f64) {
        let Some(win) = ns_window(w) else { return };
        if hidden {
            win.orderOut(None);
            return;
        }
        let Some(mtm) = MainThreadMarker::new() else { return };
        let mouse = NSEvent::mouseLocation();
        let screens = NSScreen::screens(mtm);
        let screen = screens
            .iter()
            .find(|s| {
                let f = s.frame();
                mouse.x >= f.origin.x
                    && mouse.x <= f.origin.x + f.size.width
                    && mouse.y >= f.origin.y
                    && mouse.y <= f.origin.y + f.size.height
            })
            .or_else(|| NSScreen::mainScreen(mtm));
        if let Some(screen) = screen {
            let f = screen.visibleFrame();
            let origin = NSPoint::new(f.origin.x + f.size.width / 2.0 - width / 2.0, f.origin.y + 28.0);
            win.setFrame_display(NSRect::new(origin, NSSize::new(width, height)), true);
        }
        clip_to_capsule(&win, height);
        win.orderFrontRegardless();
    }
}
