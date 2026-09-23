//! JezFlow Tauri app. Phase 2 (integration) fills in the orchestrator, commands, tray and CLI tools.

pub mod cli;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .run(tauri::generate_context!())
        .expect("error while running JezFlow");
}
