//! Tauri commands. Mirrors `api` in src/lib/api.ts — change both together.

use openflow_core::model::{AppInfo, InputDevice, Meeting};
use openflow_core::{paths, Config};
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;
use uuid::Uuid;

use crate::orchestrator::Msg;
use crate::state::{OverlayState, Snapshot};
use crate::AppState;

type CmdResult<T> = Result<T, String>;

fn parse_id(id: &str) -> CmdResult<Uuid> {
    Uuid::parse_str(id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn snapshot(state: State<'_, AppState>) -> Snapshot {
    state.shared.snapshot()
}

#[tauri::command]
pub fn set_config(state: State<'_, AppState>, config: Config) {
    let previous = {
        let mut store = state.shared.store();
        let previous = std::mem::replace(&mut store.config, config);
        store.save_config();
        previous
    };
    state.orchestrator.send(Msg::ConfigChanged { previous: Box::new(previous) });
    state.shared.emit_snapshot();
}

#[tauri::command]
pub fn delete_dictation(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.shared.store().delete(parse_id(&id)?);
    state.shared.emit_snapshot();
    Ok(())
}

#[tauri::command]
pub fn clear_history(state: State<'_, AppState>) {
    state.shared.store().clear_history();
    state.shared.emit_snapshot();
}

#[tauri::command]
pub fn set_correction(state: State<'_, AppState>, id: String, text: String) -> CmdResult<()> {
    state.shared.store().set_correction(parse_id(&id)?, &text);
    state.shared.emit_snapshot();
    Ok(())
}

#[tauri::command]
pub fn start_meeting(state: State<'_, AppState>) {
    state.orchestrator.send(Msg::StartMeeting { app: None });
}

#[tauri::command]
pub fn stop_meeting(state: State<'_, AppState>) {
    state.orchestrator.send(Msg::StopMeeting);
}

#[tauri::command]
pub fn summarize_meeting(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.orchestrator.send(Msg::Summarize(parse_id(&id)?));
    Ok(())
}

/// Title/notes/todo edits from the UI. A changed title stops automatic titling.
#[tauri::command]
pub fn update_meeting(state: State<'_, AppState>, meeting: Meeting) {
    state.shared.store().update_meeting(meeting.id, |m| {
        if m.title != meeting.title {
            m.title = meeting.title.clone();
            m.title_auto = false;
        }
        m.notes = meeting.notes.clone();
        m.todos = meeting.todos.clone();
    });
    state.shared.emit_snapshot();
}

#[tauri::command]
pub fn delete_meeting(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.shared.store().delete_meeting(parse_id(&id)?);
    state.shared.emit_snapshot();
    Ok(())
}

#[tauri::command]
pub async fn meeting_markdown(state: State<'_, AppState>, id: String) -> CmdResult<String> {
    let id = parse_id(&id)?;
    let meeting = state.shared.store().meetings.iter().find(|m| m.id == id).cloned().ok_or("no such meeting")?;
    let name = match &meeting.app {
        Some(bundle) => state.orchestrator.ask(|tx| Msg::AppInfo(bundle.clone(), tx)).await.flatten().map(|a| a.name),
        None => None,
    };
    Ok(meeting.markdown(&|bundle: &str| {
        name.clone().unwrap_or_else(|| bundle.rsplit('.').next().unwrap_or(bundle).to_string())
    }))
}

#[tauri::command]
pub fn write_text_file(path: String, text: String) -> CmdResult<()> {
    std::fs::write(path, text).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn download_model(state: State<'_, AppState>, file: String, url: String) {
    state.orchestrator.send(Msg::Download { file, url });
}

#[tauri::command]
pub fn cancel_download(state: State<'_, AppState>, file: String) {
    state.orchestrator.send(Msg::CancelDownload(file));
}

#[tauri::command]
pub fn set_model(state: State<'_, AppState>, file: String) {
    let mut config = state.shared.config();
    config.model = file;
    set_config(state, config);
}

#[tauri::command]
pub fn set_asr_version(state: State<'_, AppState>, version: String) {
    let mut config = state.shared.config();
    config.asr_version = version;
    set_config(state, config);
}

#[tauri::command]
pub async fn input_devices(state: State<'_, AppState>) -> CmdResult<Vec<InputDevice>> {
    Ok(state.orchestrator.ask(Msg::InputDevices).await.unwrap_or_default())
}

#[tauri::command]
pub async fn running_apps(state: State<'_, AppState>) -> CmdResult<Vec<AppInfo>> {
    Ok(state.orchestrator.ask(Msg::RunningApps).await.unwrap_or_default())
}

#[tauri::command]
pub async fn app_info(state: State<'_, AppState>, bundle_id: String) -> CmdResult<Option<AppInfo>> {
    Ok(state.orchestrator.ask(|tx| Msg::AppInfo(bundle_id, tx)).await.flatten())
}

#[tauri::command]
pub fn open_path(app: AppHandle, which: String) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let path = match which.as_str() {
        "config" => paths::data_dir(),
        "models" => {
            let _ = std::fs::create_dir_all(paths::models_dir());
            paths::models_dir()
        }
        "readme" => crate::readme_path(&app).ok_or("README not found")?,
        "ollama-download" => {
            return app.opener().open_url("https://ollama.com/download", None::<&str>).map_err(|e| e.to_string());
        }
        _ => return Err(format!("unknown path {which}")),
    };
    app.opener().open_path(path.display().to_string(), None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn copy_text(state: State<'_, AppState>, text: String) {
    state.orchestrator.send(Msg::CopyText(text));
}

#[tauri::command]
pub fn get_launch_at_login(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
pub fn set_launch_at_login(app: AppHandle, enabled: bool) -> CmdResult<()> {
    let l = app.autolaunch();
    if enabled { l.enable() } else { l.disable() }.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn overlay_action(state: State<'_, AppState>, action: String) {
    state.orchestrator.send(Msg::OverlayAction(action));
}

#[tauri::command]
pub fn overlay_state(state: State<'_, AppState>) -> OverlayState {
    state.shared.overlay_state()
}

/// Uncaught errors from the webviews (see src/lib/errors.ts).
#[tauri::command]
pub fn log_frontend(window: String, message: String) {
    tracing::error!(target: "frontend", "[{window}] {message}");
}

/// Page requested with `--settings <Page>`, returned once.
#[tauri::command]
pub fn initial_page(state: State<'_, AppState>) -> Option<String> {
    state.shared.runtime().initial_page.take()
}

#[tauri::command]
pub async fn ollama_status(state: State<'_, AppState>) -> CmdResult<crate::orchestrator::OllamaStatus> {
    Ok(crate::orchestrator::ollama_status(&state.shared.config()).await)
}

#[tauri::command]
pub async fn ollama_start(state: State<'_, AppState>) -> CmdResult<()> {
    state.orchestrator.ask(Msg::OllamaStart).await.unwrap_or_else(|| Err("OpenFlow is shutting down".into()))
}

#[tauri::command]
pub fn ollama_pull(state: State<'_, AppState>, model: String) {
    state.orchestrator.send(Msg::OllamaPull(model));
}

#[tauri::command]
pub fn ollama_cancel_pull(state: State<'_, AppState>, model: String) {
    state.orchestrator.send(Msg::OllamaCancelPull(model));
}
