//! JezFlow Tauri app: windows, tray, commands and the orchestrator that runs dictation and meetings.

pub mod cli;
mod commands;
mod logging;
mod meeting;
mod orchestrator;
mod state;
mod tray;
mod windows;

use std::path::PathBuf;

use jez_core::store::Store;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Manager, RunEvent, WindowEvent};

use state::{Shared, SharedRef};

pub struct AppState {
    pub shared: SharedRef,
    pub orchestrator: orchestrator::Handle,
}

/// README shipped in the bundle's resources (Help in the sidebar).
pub(crate) fn readme_path(app: &AppHandle) -> Option<PathBuf> {
    let p = app.path().resource_dir().ok()?.join("README.md");
    p.exists().then_some(p)
}

/// Standard app menu so ⌘C/⌘V/⌘Z work in text fields and ⌘Q/⌘W behave normally.
fn app_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let app_menu = Submenu::with_items(
        app,
        "JezFlow",
        true,
        &[
            &PredefinedMenuItem::about(app, Some("About JezFlow"), None)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "settings", "Settings…", true, Some("CmdOrCtrl+,"))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, Some("Hide JezFlow"))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, Some("Quit JezFlow"))?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;
    let window = Submenu::with_items(
        app,
        "Window",
        true,
        &[&PredefinedMenuItem::close_window(app, None)?, &PredefinedMenuItem::minimize(app, None)?],
    )?;
    Menu::with_items(app, &[&app_menu, &edit, &window])
}

/// `--settings [Page]` opens the window on a page.
fn settings_arg() -> Option<Option<String>> {
    let args: Vec<String> = std::env::args().collect();
    let i = args.iter().position(|a| a == "--settings")?;
    Some(args.get(i + 1).filter(|a| !a.starts_with("--")).cloned())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    logging::init();
    let shared = Shared::new(Store::load());
    let (handle, rx) = orchestrator::Handle::new();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .manage(AppState { shared: shared.clone(), orchestrator: handle.clone() })
        .menu(app_menu)
        .on_menu_event(|app, event| {
            if event.id().as_ref() == "settings" {
                windows::open_main(app, None);
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::snapshot,
            commands::set_config,
            commands::delete_dictation,
            commands::clear_history,
            commands::set_correction,
            commands::start_meeting,
            commands::stop_meeting,
            commands::summarize_meeting,
            commands::update_meeting,
            commands::delete_meeting,
            commands::meeting_markdown,
            commands::write_text_file,
            commands::download_model,
            commands::cancel_download,
            commands::set_model,
            commands::set_asr_version,
            commands::input_devices,
            commands::running_apps,
            commands::app_info,
            commands::open_path,
            commands::copy_text,
            commands::get_launch_at_login,
            commands::set_launch_at_login,
            commands::overlay_action,
            commands::overlay_state,
            commands::log_frontend,
        ])
        .setup(move |app| {
            let handle_app = app.handle().clone();
            shared.attach(handle_app.clone());
            tray::create(&handle_app, &shared)?;
            windows::setup_overlay(&handle_app);
            orchestrator::spawn(handle_app.clone(), shared.clone(), handle.clone(), rx);
            let settings = settings_arg();
            if settings.is_some() || shared.config().window_at_launch {
                windows::open_main(&handle_app, settings.flatten().as_deref());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the main window hides it (the app keeps running in the Dock and menu bar).
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building JezFlow");

    app.run(|app, event| match event {
        // Clicking the Dock icon brings the window back.
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { has_visible_windows, .. } if !has_visible_windows => windows::open_main(app, None),
        RunEvent::Exit => orchestrator::shutdown_now(),
        _ => {}
    });
}
