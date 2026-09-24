//! The app's brain (AppDelegate.swift): owns the dictation state machine, engines and meetings.
//! Everything else (hotkey, tray, commands, pill buttons) talks to it through `Msg`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::Utc;
use openflow_core::model::{AppInfo, Dictation, InputDevice, Meeting};
use openflow_core::traits::{AsrEngine, HotkeyEvent, NameSource, SystemAudio};
use openflow_core::{paths, pipeline, text_rules, wav, Config, FieldContext, SAMPLE_RATE};
use openflow_engines::cloud::{self, Cloud, Scope};
use openflow_engines::downloader::{DownloadEvent, Downloader};
use openflow_engines::live::LiveTranscriber;
use openflow_engines::meeting_engine::MeetingEngine;
use openflow_engines::ollama::{self, Ollama, OllamaModel};
use openflow_engines::polisher::Polisher;
use openflow_engines::sidecar::{Sidecar, SidecarAsr, SidecarNames, SidecarSystemAudio};
use openflow_engines::whisper_server::WhisperServer;
use openflow_platform::detector::MeetingDetector;
use openflow_platform::live_typer::LiveTyper;
use openflow_platform::Platform;
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, oneshot};
use tauri::async_runtime::JoinHandle;
use uuid::Uuid;

use crate::meeting::MeetingSession;
use crate::state::{OverlayMode, SharedRef};
use crate::tray::{self, TrayIconKind};

const SILENCE_MESSAGE: &str =
    "Microphone gave silence: pick another input in Settings (built-in is off while the lid is closed)";

pub enum Msg {
    Hotkey(HotkeyEvent),
    StartMeeting { app: Option<String> },
    StopMeeting,
    ToggleMeeting,
    Summarize(Uuid),
    /// "startMeeting" | "dismissMeeting" | "stopMeeting" from the pill.
    OverlayAction(String),
    /// The config was replaced from the UI; `previous` lets the orchestrator restart what changed.
    ConfigChanged { previous: Box<Config> },
    Download { file: String, url: String },
    CancelDownload(String),
    CopyText(String),
    InputDevices(oneshot::Sender<Vec<InputDevice>>),
    RunningApps(oneshot::Sender<Vec<AppInfo>>),
    AppInfo(String, oneshot::Sender<Option<AppInfo>>),
    /// Start Ollama (Ollama.app or `ollama serve`); replies with an error message on failure.
    OllamaStart(oneshot::Sender<Result<(), String>>),
    /// `ollama pull`; progress goes out on the `download` event with `file` = the tag.
    OllamaPull(String),
    OllamaCancelPull(String),
    /// Internal: engines finished loading / failed.
    Render,
    Quit,
}

#[derive(Clone)]
pub struct Handle {
    tx: mpsc::UnboundedSender<Msg>,
}

impl Handle {
    pub fn new() -> (Handle, mpsc::UnboundedReceiver<Msg>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Handle { tx }, rx)
    }

    pub fn send(&self, msg: Msg) {
        let _ = self.tx.send(msg);
    }

    pub async fn ask<T>(&self, make: impl FnOnce(oneshot::Sender<T>) -> Msg) -> Option<T> {
        let (tx, rx) = oneshot::channel();
        self.send(make(tx));
        rx.await.ok()
    }
}

#[derive(Debug, Clone, PartialEq)]
enum State {
    Loading,
    Idle,
    Recording,
    Processing,
    Error(String),
}

/// Per-dictation state, reset on every key-down.
#[derive(Default)]
struct Dictating {
    pressed_at: Option<Instant>,
    toggle_mode: bool,
    use_alt: bool,
    target_bundle: Option<String>,
    field: Option<FieldContext>,
    live_text: String,
}

struct MeetingRun {
    session: Arc<MeetingSession>,
    start: chrono::DateTime<Utc>,
    whisper: Option<Arc<WhisperServer>>,
}

struct Inner {
    app: AppHandle,
    shared: SharedRef,
    handle: Handle,
    platform: Platform,
    sidecar: Arc<Sidecar>,
    asr: Arc<dyn AsrEngine>,
    live: Arc<LiveTranscriber>,
    names: Arc<dyn NameSource>,
    system_audio: Arc<dyn SystemAudio>,
    typer: tokio::sync::Mutex<LiveTyper>,
    polisher: Mutex<Option<Arc<Polisher>>>,
    state: Mutex<State>,
    recording: Arc<AtomicBool>,
    dictating: Mutex<Dictating>,
    live_task: Mutex<Option<JoinHandle<()>>>,
    meeting: Mutex<Option<MeetingRun>>,
    meeting_offer: Mutex<Option<String>>,
    detector: MeetingDetector,
    downloader: Downloader,
    /// Bumped on every engine (re)start so a slow, superseded start doesn't install itself.
    polisher_generation: Mutex<u64>,
    /// `ollama serve` when we started it ourselves (not when Ollama.app runs it); stopped on quit.
    ollama_child: Mutex<Option<tokio::process::Child>>,
    /// Running pulls by tag, with their cancel flags.
    pulls: Mutex<std::collections::HashMap<String, Arc<AtomicBool>>>,
}

type Orc = Arc<Inner>;

/// Set once the orchestrator runs; `shutdown_now` stops child processes on quit.
static ORC: std::sync::OnceLock<Orc> = std::sync::OnceLock::new();

/// Stops llama-server, whisper-server, the helper and the mic. Safe to call more than once.
pub fn shutdown_now() {
    if let Some(orc) = ORC.get() {
        shutdown(orc);
    }
}

pub fn spawn(app: AppHandle, shared: SharedRef, handle: Handle, mut rx: mpsc::UnboundedReceiver<Msg>) {
    tauri::async_runtime::spawn(async move {
        let platform = Platform::native();
        let sidecar = Sidecar::new();
        let asr: Arc<dyn AsrEngine> = Arc::new(SidecarAsr(Arc::clone(&sidecar)));
        let detector = MeetingDetector::new(
            Arc::clone(&platform.apps),
            Arc::clone(&platform.devices),
            {
                let field = Arc::clone(&platform.field);
                Arc::new(move || field.frontmost_app())
            },
        );
        let app2 = app.clone();
        let downloader = Downloader::new(Arc::new(move |e: DownloadEvent| {
            let _ = app2.emit(
                "download",
                json!({ "file": e.file, "progress": e.progress, "done": e.done, "error": e.error }),
            );
            if e.done {
                if let Some(s) = app2.try_state::<crate::AppState>() {
                    s.shared.emit_snapshot();
                }
            }
        }));
        let orc: Orc = Arc::new(Inner {
            typer: tokio::sync::Mutex::new(LiveTyper::new(Arc::clone(&platform.keyboard), Arc::clone(&platform.field))),
            live: Arc::new(LiveTranscriber::new(Arc::clone(&asr))),
            names: Arc::new(SidecarNames(Arc::clone(&sidecar))),
            system_audio: Arc::new(SidecarSystemAudio(Arc::clone(&sidecar))),
            asr,
            sidecar,
            app,
            shared,
            handle,
            platform,
            polisher: Mutex::new(None),
            state: Mutex::new(State::Loading),
            recording: Arc::new(AtomicBool::new(false)),
            dictating: Mutex::new(Dictating::default()),
            live_task: Mutex::new(None),
            meeting: Mutex::new(None),
            meeting_offer: Mutex::new(None),
            detector,
            downloader,
            polisher_generation: Mutex::new(0),
            ollama_child: Mutex::new(None),
            pulls: Mutex::new(Default::default()),
        });
        let _ = ORC.set(Arc::clone(&orc));
        launch(&orc);
        while let Some(msg) = rx.recv().await {
            handle_msg(&orc, msg).await;
        }
    });
}

use tauri::Manager;

// ---------------------------------------------------------------------------------------------- launch

fn launch(orc: &Orc) {
    render(orc);
    let p = &orc.platform;
    p.field.is_trusted(true);

    // Hotkey: callbacks run on the event-tap thread, so they only forward a message.
    let h = orc.handle.clone();
    let recording = Arc::clone(&orc.recording);
    if let Err(e) = p.hotkey.start(
        orc.shared.config().hotkey_key_code,
        Arc::new(move |ev| h.send(Msg::Hotkey(ev))),
        Arc::new(move || recording.load(Ordering::SeqCst)),
    ) {
        tracing::error!("hotkey: {e:#}");
    }
    // The tap retries until Accessibility is granted; refresh the status line when that happens.
    {
        let o = Arc::clone(orc);
        tauri::async_runtime::spawn(async move {
            let mut was = false;
            loop {
                tokio::time::sleep(Duration::from_secs(3)).await;
                let now = o.platform.hotkey.is_running();
                if now != was {
                    was = now;
                    render(&o);
                }
                if now {
                    break;
                }
            }
        });
    }
    {
        let mic_level_app = orc.app.clone();
        p.mic.set_on_level(Some(Arc::new(move |v| {
            let _ = mic_level_app.emit_to("overlay", "level", v);
        })));
    }

    // Meeting detection.
    {
        let o = Arc::clone(orc);
        let dictation_mic = Arc::clone(&orc.platform.mic);
        orc.detector.set_is_recording(Arc::new(move || {
            // Our own mic use (including its 20 s keep-warm) is not a call.
            o.meeting.lock().unwrap().is_some() || dictation_mic.is_running()
        }));
        let o = Arc::clone(orc);
        orc.detector.set_on_detected(Arc::new(move |app_name| {
            let o = Arc::clone(&o);
            tauri::async_runtime::spawn(async move {
                if !o.shared.config().meeting_detect || o.meeting.lock().unwrap().is_some() {
                    return;
                }
                *o.meeting_offer.lock().unwrap() = Some(app_name.clone());
                render(&o);
                tokio::time::sleep(Duration::from_secs(45)).await;
                let expired = o.meeting_offer.lock().unwrap().as_deref() == Some(app_name.as_str());
                if expired {
                    *o.meeting_offer.lock().unwrap() = None;
                    o.detector.mark_handled();
                    render(&o);
                }
            });
        }));
        let o = Arc::clone(orc);
        orc.detector.set_on_ended(Arc::new(move || {
            *o.meeting_offer.lock().unwrap() = None;
            render(&o);
        }));
        orc.detector.start();
    }

    // Contacts/Calendar names now and hourly.
    {
        let o = Arc::clone(orc);
        tauri::async_runtime::spawn(async move {
            loop {
                refresh_external_names(&o).await;
                tokio::time::sleep(Duration::from_secs(3600)).await;
            }
        });
    }

    let o = Arc::clone(orc);
    tauri::async_runtime::spawn(async move {
        load_transcriber(&o).await;
        start_polisher(&o).await;
    });
}

async fn refresh_external_names(orc: &Orc) {
    let names = if orc.shared.config().use_contacts { orc.names.names().await } else { vec![] };
    tracing::info!("Contacts/Calendar names: {}", names.len());
    orc.shared.runtime().external_names = names;
}

async fn load_transcriber(orc: &Orc) {
    orc.shared.runtime().speech_ready = false;
    set_state(orc, State::Loading);
    let config = orc.shared.config();
    match orc.asr.load(&config.asr_version).await {
        Ok(()) => {
            orc.shared.runtime().speech_ready = true;
            set_state(orc, State::Idle);
            if let Err(e) = orc.live.set_vocabulary(&config.dictionary, &[]).await {
                tracing::error!("vocabulary: {e:#}");
            }
        }
        Err(e) => {
            tracing::error!("Speech model failed: {e:#}");
            set_state(orc, State::Error(format!("{e:#}")));
        }
    }
}

/// Starts the configured engine and warms it up. With Ollama as the engine and a GGUF installed, llama.cpp
/// is the fallback when Ollama can't start (not installed, model not pulled, server error).
async fn start_polisher(orc: &Orc) {
    let generation = {
        let mut g = orc.polisher_generation.lock().unwrap();
        *g += 1;
        *g
    };
    if let Some(p) = orc.polisher.lock().unwrap().take() {
        p.stop();
    }
    {
        let mut rt = orc.shared.runtime();
        rt.polish_ready = false;
        rt.active_backend = None;
    }
    render(orc);
    let config = orc.shared.config();
    let mut started = launch_polisher(orc, config.clone()).await;
    if let Err(e) = &started {
        if config.uses_ollama() && llamacpp_available(&config) {
            tracing::error!("Ollama unavailable ({e:#}); falling back to llama.cpp");
            let mut fallback = config.clone();
            fallback.llm_backend = "llamacpp".into();
            started = launch_polisher(orc, fallback).await;
        }
    }
    if *orc.polisher_generation.lock().unwrap() != generation {
        if let Ok(p) = started {
            p.stop();
        }
        return;
    }
    match started {
        Ok(p) => {
            let backend = if p.config().uses_ollama() { "ollama" } else { "llamacpp" };
            *orc.polisher.lock().unwrap() = Some(p);
            {
                let mut rt = orc.shared.runtime();
                rt.polish_ready = true;
                rt.active_backend = Some(backend.into());
            }
            tracing::info!("Engines ready ({backend})");
            render(orc);
            // Any finished meeting still without notes gets them now, one at a time (oldest first).
            let pending: Vec<Uuid> = orc
                .shared
                .store()
                .meetings
                .iter()
                .rev()
                .filter(|m| !m.is_live() && m.summary.is_empty() && !m.segments.is_empty())
                .map(|m| m.id)
                .collect();
            for id in pending {
                summarize(orc, id).await;
            }
        }
        Err(e) => {
            // Dictation still works without the polish model; everything is pasted verbatim.
            tracing::error!("Polish model unavailable, verbatim only: {e:#}");
            render(orc);
        }
    }
}

/// llama-server and the configured GGUF are both on disk.
fn llamacpp_available(config: &Config) -> bool {
    std::path::Path::new(&config.llama_server_path).exists() && config.model_path().exists()
}

async fn launch_polisher(orc: &Orc, config: Config) -> anyhow::Result<Arc<Polisher>> {
    let p = Arc::new(Polisher::new(config.clone(), Scope::Dictation));
    let result = async {
        p.start().await?;
        if !p.wait_until_ready(Duration::from_secs(120)).await {
            anyhow::bail!("the polish engine didn't become ready");
        }
        let examples = orc.shared.store().all_corrections(3);
        use openflow_core::traits::PolishBackend;
        // Loads the model (Ollama) and fills the prompt cache.
        p.polish(
            "warm up",
            &config.default_tones.tone,
            &config.tones,
            &config.dictionary,
            &config.memory,
            &examples,
            None,
            None,
        )
        .await?;
        anyhow::Ok(())
    }
    .await;
    match result {
        Ok(()) => Ok(p),
        Err(e) => {
            p.stop();
            Err(e)
        }
    }
}

// ---------------------------------------------------------------------------------------------- messages

async fn handle_msg(orc: &Orc, msg: Msg) {
    match msg {
        Msg::Hotkey(HotkeyEvent::Press { alt }) => hotkey_pressed(orc, alt),
        Msg::Hotkey(HotkeyEvent::Release) => hotkey_released(orc),
        Msg::Hotkey(HotkeyEvent::Escape) => cancel_recording(orc),
        Msg::StartMeeting { app } => start_meeting(orc, app),
        Msg::StopMeeting => stop_meeting(orc),
        Msg::ToggleMeeting => {
            if orc.meeting.lock().unwrap().is_none() {
                let app = orc.detector.current_call();
                start_meeting(orc, app);
            } else {
                stop_meeting(orc);
            }
        }
        Msg::Summarize(id) => {
            let o = Arc::clone(orc);
            tauri::async_runtime::spawn(async move { summarize(&o, id).await });
        }
        Msg::OverlayAction(action) => match action.as_str() {
            "startMeeting" => {
                let app = orc.meeting_offer.lock().unwrap().clone();
                start_meeting(orc, app);
            }
            "dismissMeeting" => {
                *orc.meeting_offer.lock().unwrap() = None;
                orc.detector.mark_handled();
                render(orc);
            }
            "stopMeeting" => stop_meeting(orc),
            _ => {}
        },
        Msg::ConfigChanged { previous } => config_changed(orc, &previous),
        Msg::Download { file, url } => {
            orc.downloader.start(&file, &url, &paths::models_dir());
        }
        Msg::CancelDownload(file) => orc.downloader.cancel(&file, &paths::models_dir()),
        Msg::CopyText(text) => orc.platform.keyboard.clipboard_set(&text),
        Msg::InputDevices(tx) => {
            let d = Arc::clone(&orc.platform.devices);
            tauri::async_runtime::spawn(async move {
                let _ = tx.send(tokio::task::spawn_blocking(move || d.inputs()).await.unwrap_or_default());
            });
        }
        Msg::RunningApps(tx) => {
            let a = Arc::clone(&orc.platform.apps);
            tauri::async_runtime::spawn(async move {
                let mut apps = tokio::task::spawn_blocking(move || a.running()).await.unwrap_or_default();
                apps.retain(|a| a.bundle_id != "com.himanshuat.openflow");
                apps.sort_by_key(|a| a.name.to_lowercase());
                let _ = tx.send(apps);
            });
        }
        Msg::AppInfo(bundle, tx) => {
            let a = Arc::clone(&orc.platform.apps);
            tauri::async_runtime::spawn(async move {
                let _ = tx.send(tokio::task::spawn_blocking(move || a.info(&bundle)).await.ok().flatten());
            });
        }
        Msg::OllamaStart(tx) => {
            let o = Arc::clone(orc);
            tauri::async_runtime::spawn(async move {
                let config = o.shared.config();
                let result = Ollama::new(&config.ollama_url).ensure_running(&config.ollama_path).await;
                let reply = match result {
                    Ok(child) => {
                        if child.is_some() {
                            *o.ollama_child.lock().unwrap() = child;
                        }
                        // Ollama is the engine and just came up: load the model now.
                        if config.uses_ollama() && !o.shared.runtime().polish_ready {
                            let o2 = Arc::clone(&o);
                            tauri::async_runtime::spawn(async move { start_polisher(&o2).await });
                        }
                        Ok(())
                    }
                    Err(e) => Err(format!("{e:#}")),
                };
                let _ = tx.send(reply);
            });
        }
        Msg::OllamaPull(tag) => ollama_pull(orc, tag),
        Msg::OllamaCancelPull(tag) => {
            if let Some(flag) = orc.pulls.lock().unwrap().get(&tag) {
                flag.store(true, Ordering::SeqCst);
            }
        }
        Msg::Render => render(orc),
        Msg::Quit => orc.app.exit(0), // RunEvent::Exit then calls shutdown_now
    }
}

fn config_changed(orc: &Orc, previous: &Config) {
    let now = orc.shared.config();
    let engine_changed = now.llm_backend != previous.llm_backend
        || now.ollama_url != previous.ollama_url
        || now.ollama_model != previous.ollama_model
        || now.ollama_path != previous.ollama_path
        || now.model != previous.model
        || now.llama_server_path != previous.llama_server_path
        || now.llama_port != previous.llama_port;
    if engine_changed {
        let o = Arc::clone(orc);
        tauri::async_runtime::spawn(async move { start_polisher(&o).await });
    } else if let Some(p) = orc.polisher.lock().unwrap().as_ref() {
        // Tones, dictionary, memory and cloud settings are read per request; hand the polisher the new config,
        // keeping the engine it actually runs on (llama.cpp when it is Ollama's fallback).
        let mut cfg = now.clone();
        cfg.llm_backend = p.config().llm_backend;
        p.set_config(cfg);
    }
    if now.asr_version != previous.asr_version {
        let o = Arc::clone(orc);
        tauri::async_runtime::spawn(async move { load_transcriber(&o).await });
    } else if now.dictionary != previous.dictionary {
        let (live, dict) = (Arc::clone(&orc.live), now.dictionary.clone());
        tauri::async_runtime::spawn(async move {
            let _ = live.set_vocabulary(&dict, &[]).await;
        });
    }
    if now.use_contacts != previous.use_contacts {
        let o = Arc::clone(orc);
        tauri::async_runtime::spawn(async move { refresh_external_names(&o).await });
    }
    render(orc);
}

/// A clean quit also stops llama-server, whisper-server and the helper.
fn shutdown(orc: &Orc) {
    orc.platform.mic.shutdown();
    if let Some(run) = orc.meeting.lock().unwrap().take() {
        run.session.stop();
        if let Some(w) = run.whisper {
            w.stop();
        }
    }
    if let Some(p) = orc.polisher.lock().unwrap().take() {
        p.stop();
    }
    orc.detector.stop();
    orc.sidecar.shutdown();
    // Dropping the child kills `ollama serve` if we started it (kill_on_drop); Ollama.app keeps running.
    orc.ollama_child.lock().unwrap().take();
}

/// Ollama server status for the Models page.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaStatus {
    pub installed: bool,
    pub running: bool,
    pub version: Option<String>,
    pub url: String,
    pub models: Vec<OllamaModel>,
    pub error: Option<String>,
}

pub async fn ollama_status(config: &Config) -> OllamaStatus {
    let client = Ollama::new(&config.ollama_url);
    let installed = ollama::app_installed() || ollama::find_binary(&config.ollama_path).is_some();
    let (version, error) = match client.version().await {
        Ok(v) => (Some(v), None),
        Err(e) => (None, Some(format!("{e:#}"))),
    };
    let models = if version.is_some() { client.models().await.unwrap_or_default() } else { vec![] };
    OllamaStatus { installed, running: version.is_some(), version, url: client.url().to_string(), models, error }
}

/// Pulls a model, reporting progress like a GGUF download. Starts Ollama first when it isn't running.
fn ollama_pull(orc: &Orc, tag: String) {
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut pulls = orc.pulls.lock().unwrap();
        if pulls.contains_key(&tag) {
            return;
        }
        pulls.insert(tag.clone(), Arc::clone(&cancel));
    }
    let o = Arc::clone(orc);
    tauri::async_runtime::spawn(async move {
        let emit = |progress: Option<f64>, done: bool, error: Option<String>| {
            let _ = o.app.emit("download", json!({ "file": tag, "progress": progress, "done": done, "error": error }));
        };
        emit(Some(0.0), false, None);
        let config = o.shared.config();
        let client = Ollama::new(&config.ollama_url);
        let result = async {
            if let Some(child) = client.ensure_running(&config.ollama_path).await? {
                *o.ollama_child.lock().unwrap() = Some(child);
            }
            client.pull(&tag, &|p| emit(p, false, None), Arc::clone(&cancel)).await
        }
        .await;
        o.pulls.lock().unwrap().remove(&tag);
        match result {
            Ok(()) => {
                tracing::info!("Pulled {tag}");
                emit(Some(1.0), true, None);
                let config = o.shared.config();
                if config.uses_ollama() && config.ollama_model == tag && !o.shared.runtime().polish_ready {
                    start_polisher(&o).await;
                }
            }
            // A cancelled pull reports nothing, like a cancelled GGUF download.
            Err(_) if cancel.load(Ordering::SeqCst) => emit(None, true, None),
            Err(e) => emit(None, true, Some(format!("{e:#}"))),
        }
        o.shared.emit_snapshot();
    });
}

// ---------------------------------------------------------------------------------------------- rendering

fn state(orc: &Orc) -> State {
    orc.state.lock().unwrap().clone()
}

fn set_state(orc: &Orc, s: State) {
    orc.recording.store(s == State::Recording, Ordering::SeqCst);
    *orc.state.lock().unwrap() = s;
    render(orc);
}

/// Error states clear themselves after `after`, unless something else happened meanwhile.
fn clear_error_after(orc: &Orc, after: Duration) {
    let o = Arc::clone(orc);
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(after).await;
        if matches!(state(&o), State::Error(_)) {
            set_state(&o, State::Idle);
        }
    });
}

/// Status line, tray icon and pill for the current state (AppDelegate.render).
fn render(orc: &Orc) {
    let st = state(orc);
    let toggle = orc.dictating.lock().unwrap().toggle_mode;
    let (icon, text) = match &st {
        State::Loading => (TrayIconKind::Loading, "Loading models…".to_string()),
        State::Idle => {
            let text = if !orc.platform.hotkey.is_running() {
                "Grant Accessibility to enable the hotkey"
            } else if orc.polisher.lock().unwrap().is_none() {
                "Ready (verbatim only — polish model not loaded)"
            } else {
                "Ready — hold ⌘; to dictate"
            };
            (TrayIconKind::Idle, text.to_string())
        }
        State::Recording => (
            TrayIconKind::Recording,
            if toggle { "Recording (tap ⌘; to stop, Esc to cancel)" } else { "Recording (release to finish)" }.to_string(),
        ),
        State::Processing => (TrayIconKind::Processing, "Transcribing…".to_string()),
        State::Error(m) => (TrayIconKind::Error, format!("Error: {m}")),
    };
    tray::set_icon(&orc.app, icon);
    orc.shared.set_status(text);
    orc.shared.emit_snapshot();

    let mode = if !orc.shared.config().overlay {
        OverlayMode::Hidden
    } else {
        match st {
            State::Recording => OverlayMode::Listening,
            State::Processing => OverlayMode::Processing,
            State::Error(m) => OverlayMode::Message { text: m },
            _ => {
                if let Some(run) = orc.meeting.lock().unwrap().as_ref() {
                    OverlayMode::MeetingLive { start: openflow_core::apple_date::to_reference(&run.start) }
                } else if let Some(app) = orc.meeting_offer.lock().unwrap().clone() {
                    OverlayMode::MeetingOffer { app }
                } else {
                    OverlayMode::Hidden
                }
            }
        }
    };
    orc.shared.set_overlay(mode, None);
}

/// A transient message in the pill (hidden again after 2.5 s).
fn show_message(orc: &Orc, text: &str) {
    if !orc.shared.config().overlay {
        return;
    }
    let mode = OverlayMode::Message { text: text.to_string() };
    orc.shared.set_overlay(mode.clone(), None);
    let o = Arc::clone(orc);
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(2500)).await;
        if o.shared.overlay_state().mode == mode {
            o.shared.set_overlay(OverlayMode::Hidden, None);
        }
    });
}

// ---------------------------------------------------------------------------------------------- dictation

fn hotkey_pressed(orc: &Orc, alt: bool) {
    match state(orc) {
        State::Idle => begin_recording(orc, alt),
        State::Recording if orc.dictating.lock().unwrap().toggle_mode => finish_recording(orc),
        _ => {}
    }
}

fn hotkey_released(orc: &Orc) {
    if state(orc) != State::Recording {
        return;
    }
    let threshold = orc.shared.config().tap_threshold;
    let finish = {
        let mut d = orc.dictating.lock().unwrap();
        let Some(pressed_at) = d.pressed_at else { return };
        if d.toggle_mode {
            return;
        }
        if pressed_at.elapsed().as_secs_f64() < threshold {
            d.toggle_mode = true; // quick tap: keep recording until the next tap
            false
        } else {
            true // held: release ends it
        }
    };
    if finish {
        finish_recording(orc);
    } else {
        render(orc);
    }
}

/// Live text goes into the field only where the field can be read back (cursor available): that is what makes
/// the swap exact. Elsewhere (browsers, Electron apps) it shows in the pill and only the final text is pasted.
fn live_text_allowed(orc: &Orc, config: &Config) -> bool {
    let d = orc.dictating.lock().unwrap();
    let excluded = d.target_bundle.as_ref().is_some_and(|b| config.live_text_excluded.contains(b));
    !excluded && d.field.as_ref().is_some_and(|f| f.cursor.is_some())
}

fn begin_recording(orc: &Orc, alt: bool) {
    let config = orc.shared.config();
    let target = orc.platform.field.frontmost_app().map(|a| a.bundle_id);
    let field = config.field_context.then(|| orc.platform.field.capture());
    {
        let mut d = orc.dictating.lock().unwrap();
        *d = Dictating { use_alt: alt, target_bundle: target, field: field.clone(), ..Default::default() };
    }
    match orc.platform.mic.start(&config.microphone) {
        Ok(()) => {
            orc.dictating.lock().unwrap().pressed_at = Some(Instant::now()); // after start(), so its cost never counts as holding
            if config.sounds {
                orc.platform.sounds.start_cue();
            }
            set_state(orc, State::Recording);
            start_live(orc);
        }
        Err(e) => set_state(orc, State::Error(format!("{e:#}"))),
    }
}

/// Re-decodes the audio so far every few hundred ms so the transcript is ready on release and, if enabled,
/// appears in the field while talking.
fn start_live(orc: &Orc) {
    if let Some(t) = orc.live_task.lock().unwrap().take() {
        t.abort();
    }
    let o = Arc::clone(orc);
    let task = tauri::async_runtime::spawn(async move {
        let config = o.shared.config();
        let field = o.dictating.lock().unwrap().field.clone();
        // Waits for a retract/finish from the previous dictation still holding the typer, then resets it.
        o.typer.lock().await.begin(field.as_ref());
        // Names visible where you're typing, plus Contacts/Calendar names, are boosted for this dictation.
        let mut extra = field
            .as_ref()
            .map(|f| text_rules::context_names(&[f.window_title.as_str(), &f.before, &f.after].join(" "), 15))
            .unwrap_or_default();
        extra.extend(o.shared.runtime().external_names.clone());
        let _ = o.live.set_vocabulary(&config.dictionary, &extra).await;
        o.live.begin_live().await;
        let lead = if field.as_ref().is_some_and(|f| f.needs_leading_space()) { " " } else { "" };
        let allowed = live_text_allowed(&o, &config);
        while o.recording.load(Ordering::SeqCst) {
            let started = Instant::now();
            let snap = o.platform.mic.snapshot();
            if snap.len() > SAMPLE_RATE / 2 {
                if let Ok(text) = o.live.live_update(&snap).await {
                    o.dictating.lock().unwrap().live_text = text.clone();
                    if config.live_text && o.recording.load(Ordering::SeqCst) {
                        if allowed {
                            o.typer.lock().await.update(&format!("{lead}{text}")).await;
                        } else if config.overlay {
                            o.shared.set_overlay(OverlayMode::Listening, Some(text));
                        }
                    }
                }
            }
            // Aim for an update every ~400 ms including the decode itself; never spin faster than 120 ms.
            let spent = started.elapsed().as_millis() as u64;
            tokio::time::sleep(Duration::from_millis(400u64.saturating_sub(spent).max(120))).await;
        }
    });
    *orc.live_task.lock().unwrap() = Some(task);
}

fn cancel_recording(orc: &Orc) {
    if state(orc) != State::Recording {
        return;
    }
    let samples = orc.platform.mic.stop();
    tracing::info!("Cancelled after {} ms", samples.len() / 16);
    if let Some(t) = orc.live_task.lock().unwrap().take() {
        t.abort();
    }
    set_state(orc, State::Idle);
    let o = Arc::clone(orc);
    tauri::async_runtime::spawn(async move {
        o.live.begin_live().await;
        o.typer.lock().await.retract().await;
    });
}

fn resolve_tone(orc: &Orc, config: &Config) -> String {
    if let Some(t) = orc.shared.runtime().tone_override.clone() {
        return t;
    }
    let d = orc.dictating.lock().unwrap();
    let t = config.tones_for(d.target_bundle.as_deref());
    if d.use_alt { t.alt.clone() } else { t.tone.clone() }
}

fn finish_recording(orc: &Orc) {
    let samples = orc.platform.mic.stop();
    let config = orc.shared.config();
    if config.sounds {
        orc.platform.sounds.stop_cue();
    }
    set_state(orc, State::Processing);
    let tone = resolve_tone(orc, &config);
    let pending = orc.live_task.lock().unwrap().take();
    let o = Arc::clone(orc);
    tauri::async_runtime::spawn(async move {
        // Let an in-flight hypothesis decode finish first (the loop exits because recording is now false).
        if let Some(p) = pending {
            let _ = p.await;
        }
        if let Err(e) = finish_dictation(&o, samples, &tone, &config).await {
            o.typer.lock().await.retract().await;
            tracing::error!("Dictation failed: {e:#}");
            set_state(&o, State::Error(format!("{e:#}")));
            clear_error_after(&o, Duration::from_secs(4));
        }
    });
}

async fn finish_dictation(orc: &Orc, samples: Vec<f32>, tone: &str, config: &Config) -> anyhow::Result<()> {
    // The Swift app compared the signed maximum; keep that behaviour.
    let peak = samples.iter().copied().fold(f32::MIN, f32::max).max(0.0);
    if samples.len() <= SAMPLE_RATE / 4 {
        tracing::error!("Nothing captured: {} samples", samples.len());
        orc.typer.lock().await.retract().await;
        orc.live.begin_live().await;
        set_state(orc, State::Idle);
        return Ok(());
    }
    if peak <= 1e-5 {
        tracing::error!("Silent recording ({} ms)", samples.len() / 16);
        orc.typer.lock().await.retract().await;
        orc.live.begin_live().await;
        set_state(orc, State::Error(SILENCE_MESSAGE.into()));
        clear_error_after(orc, Duration::from_secs(5));
        return Ok(());
    }
    let t0 = Instant::now();
    let (live_text, field, target) = {
        let d = orc.dictating.lock().unwrap();
        (d.live_text.clone(), d.field.clone(), d.target_bundle.clone())
    };
    // Long dictations, and any with a self-correction, go to the cloud in "long" mode.
    let is_long = samples.len() > 15 * SAMPLE_RATE || pipeline::has_correction_signal(&live_text, &config.learned_signals);
    let polisher = orc.polisher.lock().unwrap().clone();
    if let Some(p) = &polisher {
        p.set_current_is_long(is_long);
    }
    let cloud = Cloud::new(config, Scope::Dictation, is_long);
    tracing::info!(
        "Route: {} ({} s, cloud {}, dictation mode {})",
        if cloud.is_some() { "cloud" } else { "local" },
        samples.len() / SAMPLE_RATE,
        if config.cloud_enabled { "on" } else { "off" },
        config.cloud_dictation
    );
    // The local final decode (the fallback) and the cloud request run at the same time.
    let local = orc.live.live_finish(&samples);
    let remote = async {
        match &cloud {
            Some(c) => match c.transcribe(&samples, None).await {
                Ok(t) => t,
                Err(e) => {
                    tracing::error!("Cloud speech failed, using local: {e:#}");
                    show_message(orc, "Cloud unavailable, used local models");
                    String::new()
                }
            },
            None => String::new(),
        }
    };
    let (local, mut raw) = tokio::join!(local, remote);
    let local = local?;
    if raw.is_empty() {
        raw = local;
    }
    let t1 = Instant::now();
    if raw.is_empty() {
        tracing::error!("Empty transcript from {} ms of audio", samples.len() / 16);
        orc.typer.lock().await.retract().await;
        if peak < 1e-6 {
            set_state(orc, State::Error(SILENCE_MESSAGE.into()));
            clear_error_after(orc, Duration::from_secs(5));
        } else {
            set_state(orc, State::Idle);
        }
        return Ok(());
    }
    let prefix = if field.as_ref().is_some_and(|f| f.needs_leading_space()) { " " } else { "" };
    let examples = orc.shared.store().all_corrections(3);
    let backend = polisher.as_deref().map(|p| p as &dyn openflow_core::traits::PolishBackend);
    let mut text = pipeline::run(&raw, tone, config, backend, &examples, field.as_ref(), None).await?;
    tracing::info!(
        "Timing: audio {} ms, asr {} ms, polish {} ms, {} words",
        samples.len() / 16,
        t1.duration_since(t0).as_millis(),
        t1.elapsed().as_millis(),
        raw.split(' ').count()
    );
    if !prefix.is_empty() && !text.starts_with('\n') {
        text = format!("{prefix}{text}");
    }
    {
        let mut typer = orc.typer.lock().await;
        if typer.typed() == text {
            typer.forget(); // nothing changed: leave the live text as it is
        } else {
            typer.finish(&text).await; // backspaces, paste, clipboard back
        }
    }
    let mut d = Dictation {
        id: Uuid::new_v4(),
        date: Utc::now(),
        text: text.clone(),
        raw: raw.clone(),
        app: target,
        tone: tone.to_string(),
        seconds: samples.len() as f64 / SAMPLE_RATE as f64,
        clip: None,
        correction: None,
    };
    if config.keep_clips {
        let name = format!("{}.wav", crate::state::upper(&d.id));
        match wav::write_f32(&paths::clips_dir().join(&name), &samples) {
            Ok(()) => d.clip = Some(name),
            Err(e) => tracing::error!("Clip not saved: {e:#}"),
        }
    }
    orc.shared.store().remember(d);
    tracing::info!("[{tone}] {raw} -> {text}");
    set_state(orc, State::Idle);
    Ok(())
}

// ---------------------------------------------------------------------------------------------- meetings

fn start_meeting(orc: &Orc, app: Option<String>) {
    if orc.meeting.lock().unwrap().is_some() {
        return;
    }
    *orc.meeting_offer.lock().unwrap() = None;
    let config = orc.shared.config();
    let now = chrono::Local::now();
    let title = format!("{} · {}", app.as_ref().map(|a| format!("{a} call")).unwrap_or_else(|| "Meeting".into()), now.format("%a %-d %b, %-I:%M %p"));
    let meeting = Meeting::new(title, app.clone(), Utc::now());
    let id = meeting.id;
    {
        let mut store = orc.shared.store();
        store.add_meeting(meeting);
        store.live_meeting_id = Some(id);
    }
    let mut whisper = None;
    if config.meeting_far_engine == "whisper" && Cloud::new(&config, Scope::Meetings, false).is_none() {
        let w = Arc::new(WhisperServer::new(config.clone()));
        if w.available() {
            let w2 = Arc::clone(&w);
            tauri::async_runtime::spawn(async move {
                if let Err(e) = w2.start().await {
                    tracing::error!("whisper-server: {e:#}");
                }
            });
            whisper = Some(w);
        } else {
            tracing::info!("Whisper not available yet (binary or model missing); using Parakeet for the far side");
        }
    }
    let engine = Arc::new(MeetingEngine::new(Arc::clone(&orc.asr), config.clone(), whisper.clone()));
    let session = MeetingSession::new(
        id,
        orc.shared.clone(),
        engine,
        (orc.platform.new_mic)(),
        Arc::clone(&orc.platform.devices),
        Arc::clone(&orc.system_audio),
    );
    // Known call app: tap only its audio.
    let tap_app = if app.is_some() {
        let _ = orc.detector.current_call();
        orc.detector.current_call_bundle()
    } else {
        String::new()
    };
    match session.start(&config.microphone, &tap_app) {
        Ok(()) => {
            let system_ok = session.system_audio_working.load(Ordering::SeqCst);
            *orc.meeting.lock().unwrap() = Some(MeetingRun { session, start: Utc::now(), whisper });
            // Notes refresh every 3 minutes while the meeting runs, when enough new words arrived.
            let o = Arc::clone(orc);
            tauri::async_runtime::spawn(async move {
                let mut last_words = 0usize;
                loop {
                    tokio::time::sleep(Duration::from_secs(180)).await;
                    let live = o.meeting.lock().unwrap().as_ref().is_some_and(|r| r.session.meeting_id == id);
                    if !live {
                        break;
                    }
                    let words = o.shared.store().meetings.iter().find(|m| m.id == id).map(|m| m.words()).unwrap_or(0);
                    if words.saturating_sub(last_words) >= 40 {
                        last_words = words;
                        summarize(&o, id).await;
                    }
                }
            });
            render(orc);
            if !system_ok {
                show_message(orc, "Only your mic is captured: allow System Audio Recording");
            }
        }
        Err(e) => {
            {
                let mut store = orc.shared.store();
                store.finish_meeting(id);
                store.delete_meeting(id);
            }
            if let Some(w) = whisper {
                w.stop();
            }
            show_message(orc, &format!("Meeting notes failed: {e:#}"));
        }
    }
    orc.shared.emit_snapshot();
}

fn stop_meeting(orc: &Orc) {
    let Some(run) = orc.meeting.lock().unwrap().take() else { return };
    let id = run.session.meeting_id;
    if let Some(w) = &run.whisper {
        w.stop();
    }
    orc.detector.mark_handled();
    render(orc);
    run.session.stop(); // remaining utterances drain, then the store marks it finished
    let o = Arc::clone(orc);
    tauri::async_runtime::spawn(async move {
        loop {
            let finished = o.shared.store().meetings.iter().find(|m| m.id == id).map(|m| m.end.is_some()).unwrap_or(true);
            if finished {
                break;
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        summarize(&o, id).await;
    });
}

/// Notes (summary, decisions, action items, your to-dos) and an automatic title.
async fn summarize(orc: &Orc, id: Uuid) {
    let transcript = {
        let store = orc.shared.store();
        let Some(m) = store.meetings.iter().find(|m| m.id == id) else { return };
        if m.segments.is_empty() {
            return;
        }
        m.transcript()
    };
    if orc.polisher.lock().unwrap().is_none() {
        return;
    }
    if !orc.shared.runtime().summarizing.insert(id) {
        return;
    }
    orc.shared.emit_snapshot();
    let mut config = orc.shared.config();
    // Notes are written by the engine that actually loaded (llama.cpp when it is Ollama's fallback).
    let active = orc.shared.runtime().active_backend.clone();
    if let Some(b) = active {
        config.llm_backend = b;
    }
    // Notes model: the cloud when on; otherwise the bigger notes model (Ollama loads it on demand; llama.cpp
    // gets a second server just for this), falling back to the polish model.
    let mut writer = Arc::new(Polisher::new(config.clone(), Scope::Meetings));
    let mut transient: Option<Arc<Polisher>> = None;
    let cloud_notes = cloud::routes(&config, Scope::Meetings, false);
    if !cloud_notes && config.uses_ollama() && !config.ollama_notes_model.is_empty() {
        let ollama = Ollama::new(&config.ollama_url);
        if ollama.has_model(&config.ollama_notes_model).await.unwrap_or(false) {
            let mut cfg = config.clone();
            cfg.ollama_model = config.ollama_notes_model.clone();
            writer = Arc::new(Polisher::new(cfg, Scope::Meetings));
        } else {
            tracing::info!("Notes model {} not pulled; using {}", config.ollama_notes_model, config.ollama_model);
        }
    } else if !cloud_notes
        && !config.uses_ollama()
        && !config.notes_model.is_empty()
        && config.notes_model != config.model
        && paths::models_dir().join(&config.notes_model).exists()
    {
        let mut cfg = config.clone();
        cfg.model = config.notes_model.clone();
        cfg.llama_port = 8767;
        let p = Arc::new(Polisher::new(cfg, Scope::Meetings));
        match p.start().await {
            Ok(()) if p.wait_until_ready(Duration::from_secs(180)).await => {
                transient = Some(Arc::clone(&p));
                writer = p;
            }
            Ok(()) => p.stop(),
            Err(e) => tracing::error!("Notes model failed to start: {e:#}"),
        }
    }
    let traditional = config.traditional_chinese();
    match writer.summarize_meeting(&transcript).await {
        Ok(summary) => {
            let summary = text_rules::chinese_script(&summary, traditional);
            let title_auto = {
                let mut store = orc.shared.store();
                store.update_meeting(id, |m| {
                    m.summary = summary.clone();
                    m.refresh_todos(&summary);
                });
                store.meetings.iter().find(|m| m.id == id).is_some_and(|m| m.title_auto)
            };
            if title_auto {
                if let Ok(title) = writer.meeting_title(&summary).await {
                    if !title.is_empty() {
                        let title = text_rules::chinese_script(&title, traditional);
                        orc.shared.store().update_meeting(id, |m| m.title = title);
                    }
                }
            }
        }
        Err(e) => tracing::error!("Summary failed: {e:#}"),
    }
    if let Some(t) = transient {
        t.stop();
    }
    orc.shared.runtime().summarizing.remove(&id);
    orc.shared.emit_snapshot();
}
