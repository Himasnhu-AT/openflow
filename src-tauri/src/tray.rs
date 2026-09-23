//! Menu-bar item: status line, Tone and History submenus, meeting toggle, open, quit (AppDelegate.buildMenu).
//! Tauri can't rebuild a menu when it opens, so it is rebuilt whenever the snapshot changes.

use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager, Wry};

use crate::orchestrator::Msg;
use crate::state::{Shared, Snapshot};

pub const TRAY_ID: &str = "jezflow";

/// Tray icon per status, as SF Symbols in the Swift app.
pub enum TrayIconKind {
    Loading,
    Idle,
    Recording,
    Processing,
    Error,
}

fn icon(kind: &TrayIconKind) -> Image<'static> {
    let bytes: &'static [u8] = match kind {
        TrayIconKind::Loading => include_bytes!("../icons/tray/hourglass.png"),
        TrayIconKind::Idle => include_bytes!("../icons/tray/mic.png"),
        TrayIconKind::Recording => include_bytes!("../icons/tray/mic-fill.png"),
        TrayIconKind::Processing => include_bytes!("../icons/tray/ellipsis.png"),
        TrayIconKind::Error => include_bytes!("../icons/tray/warning.png"),
    };
    Image::from_bytes(bytes).expect("tray icon png")
}

pub fn set_icon(app: &AppHandle, kind: TrayIconKind) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_icon(Some(icon(&kind)));
        let _ = tray.set_icon_as_template(true);
    }
}

pub fn create(app: &AppHandle, shared: &Shared) -> tauri::Result<TrayIcon> {
    let menu = build_menu(app, shared, &shared.snapshot())?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon(&TrayIconKind::Loading))
        .icon_as_template(true)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| handle(app, event.id().as_ref()))
        .build(app)
}

pub fn refresh(app: &AppHandle, shared: &Shared, snap: &Snapshot) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    if let Ok(menu) = build_menu(app, shared, snap) {
        let _ = tray.set_menu(Some(menu));
    }
}

fn truncate(s: &str, n: usize) -> String {
    let one_line = s.replace('\n', " ");
    if one_line.chars().count() > n {
        format!("{}…", one_line.chars().take(n).collect::<String>())
    } else {
        one_line
    }
}

fn capitalized(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn build_menu(app: &AppHandle, shared: &Shared, snap: &Snapshot) -> tauri::Result<Menu<Wry>> {
    let status = MenuItem::with_id(app, "status", &snap.status, false, None::<&str>)?;

    let override_tone = shared.runtime().tone_override.clone();
    let tone = Submenu::with_id(app, "tone", "Tone", true)?;
    tone.append(&CheckMenuItem::with_id(
        app,
        "tone:auto",
        "Automatic (per app)",
        true,
        override_tone.is_none(),
        None::<&str>,
    )?)?;
    tone.append(&PredefinedMenuItem::separator(app)?)?;
    for name in snap.config.tones.keys() {
        let checked = override_tone.as_deref() == Some(name.as_str());
        tone.append(&CheckMenuItem::with_id(
            app,
            format!("tone:{name}"),
            capitalized(name),
            true,
            checked,
            None::<&str>,
        )?)?;
    }

    let history = Submenu::with_id(app, "history", "History", true)?;
    if snap.history.is_empty() {
        history.append(&MenuItem::with_id(app, "hist:none", "No dictations yet", false, None::<&str>)?)?;
    } else {
        for (i, d) in snap.history.iter().take(15).enumerate() {
            let accel = (i < 9).then(|| format!("{}", i + 1));
            history.append(&MenuItem::with_id(app, format!("hist:{i}"), truncate(&d.text, 70), true, accel.as_deref())?)?;
        }
    }
    history.append(&PredefinedMenuItem::separator(app)?)?;
    history.append(&MenuItem::with_id(app, "clear-history", "Clear History", true, None::<&str>)?)?;

    let meeting_title = if snap.live_meeting_id.is_some() { "Stop Meeting Notes" } else { "Start Meeting Notes" };
    let meeting = MenuItem::with_id(app, "meeting", meeting_title, true, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Open JezFlow…", true, Some("CmdOrCtrl+,"))?;
    let quit = MenuItem::with_id(app, "quit", "Quit JezFlow", true, Some("CmdOrCtrl+Q"))?;

    Menu::with_items(
        app,
        &[
            &status,
            &PredefinedMenuItem::separator(app)?,
            &tone,
            &history,
            &PredefinedMenuItem::separator(app)?,
            &meeting,
            &open,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )
}

fn handle(app: &AppHandle, id: &str) {
    let shared = app.state::<crate::AppState>().shared.clone();
    let orchestrator = app.state::<crate::AppState>().orchestrator.clone();
    match id {
        "tone:auto" => {
            shared.runtime().tone_override = None;
            shared.emit_snapshot();
        }
        "clear-history" => {
            shared.store().clear_history();
            shared.emit_snapshot();
        }
        "meeting" => orchestrator.send(Msg::ToggleMeeting),
        "open" => crate::windows::open_main(app, None),
        "quit" => orchestrator.send(Msg::Quit),
        _ if id.starts_with("tone:") => {
            shared.runtime().tone_override = Some(id["tone:".len()..].to_string());
            shared.emit_snapshot();
        }
        _ if id.starts_with("hist:") => {
            let text = id["hist:".len()..]
                .parse::<usize>()
                .ok()
                .and_then(|i| shared.store().history.get(i).map(|d| d.text.clone()));
            if let Some(text) = text {
                orchestrator.send(Msg::CopyText(text));
                shared.set_status("Copied to clipboard");
                shared.emit_snapshot();
            }
        }
        _ => {}
    }
}
