//! Shared app state: the persistent `Store` plus runtime flags, and the `Snapshot` the UI renders.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard};

use openflow_core::catalog::{self, CatalogModel};
use openflow_core::model::{Dictation, Meeting, Stats};
use openflow_core::store::Store;
use openflow_core::{paths, Config};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

/// Mirrors `OverlayMode` in src/lib/api.ts.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum OverlayMode {
    Hidden,
    Listening,
    Processing,
    Message { text: String },
    MeetingOffer { app: String },
    /// `start` in Apple-reference seconds.
    MeetingLive { start: f64 },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayState {
    pub mode: OverlayMode,
    pub live_text: String,
}

impl Default for OverlayState {
    fn default() -> Self {
        Self { mode: OverlayMode::Hidden, live_text: String::new() }
    }
}

/// Things that are not persisted.
#[derive(Debug, Default)]
pub struct Runtime {
    pub status: String,
    pub speech_ready: bool,
    pub polish_ready: bool,
    pub summarizing: BTreeSet<Uuid>,
    /// Contacts/Calendar names (boosted in speech recognition; never saved).
    pub external_names: Vec<String>,
    /// Sticky tone chosen from the tray; None = automatic per app.
    pub tone_override: Option<String>,
    /// `--settings <Page>`: handed to the main window once it asks.
    pub initial_page: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub config: Config,
    pub history: Vec<Dictation>,
    pub stats: Stats,
    pub wpm: u64,
    pub streak: u32,
    pub meetings: Vec<Meeting>,
    pub live_meeting_id: Option<String>,
    pub summarizing: Vec<String>,
    pub status: String,
    pub speech_ready: bool,
    pub polish_ready: bool,
    pub correction_count: usize,
    pub clips_present: Vec<String>,
    pub installed_models: Vec<String>,
    pub catalog: Vec<CatalogModel>,
    pub whisper_catalog: CatalogModel,
    pub whisper_server_found: bool,
    pub whisper_model_installed: bool,
    pub data_dir: String,
}

pub struct Shared {
    store: Mutex<Store>,
    runtime: Mutex<Runtime>,
    overlay: Mutex<OverlayState>,
    app: Mutex<Option<AppHandle>>,
}

pub type SharedRef = Arc<Shared>;

pub fn upper(id: &Uuid) -> String {
    id.hyphenated().to_string().to_uppercase()
}

impl Shared {
    pub fn new(store: Store) -> SharedRef {
        Arc::new(Shared {
            store: Mutex::new(store),
            runtime: Mutex::new(Runtime { status: "Loading…".into(), ..Default::default() }),
            overlay: Mutex::new(OverlayState::default()),
            app: Mutex::new(None),
        })
    }

    pub fn attach(&self, app: AppHandle) {
        *self.app.lock().unwrap() = Some(app);
    }

    pub fn app(&self) -> Option<AppHandle> {
        self.app.lock().unwrap().clone()
    }

    pub fn store(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn runtime(&self) -> MutexGuard<'_, Runtime> {
        self.runtime.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn config(&self) -> Config {
        self.store().config.clone()
    }

    pub fn overlay_state(&self) -> OverlayState {
        self.overlay.lock().unwrap().clone()
    }

    pub fn snapshot(&self) -> Snapshot {
        let store = self.store();
        let rt = self.runtime();
        let config = store.config.clone();
        let whisper_server_found = std::path::Path::new(&config.whisper_server_path).exists();
        Snapshot {
            history: store.history.clone(),
            stats: store.stats.clone(),
            wpm: store.words_per_minute(),
            streak: store.streak(),
            meetings: store.meetings.clone(),
            live_meeting_id: store.live_meeting_id.as_ref().map(upper),
            summarizing: rt.summarizing.iter().map(upper).collect(),
            status: rt.status.clone(),
            speech_ready: rt.speech_ready,
            polish_ready: rt.polish_ready,
            correction_count: store.correction_count(),
            clips_present: store.history.iter().filter(|d| store.has_clip(d)).map(|d| upper(&d.id)).collect(),
            installed_models: catalog::installed_models(),
            catalog: catalog::polish_models(),
            whisper_catalog: catalog::whisper(),
            whisper_server_found,
            whisper_model_installed: paths::models_dir().join(&config.whisper_model).exists(),
            data_dir: paths::data_dir().display().to_string(),
            config,
        }
    }

    /// Pushes the full snapshot to every window and refreshes the tray.
    pub fn emit_snapshot(&self) {
        let Some(app) = self.app() else { return };
        let snap = self.snapshot();
        crate::tray::refresh(&app, self, &snap);
        let _ = app.emit("snapshot", snap);
    }

    pub fn set_status(&self, status: impl Into<String>) {
        self.runtime().status = status.into();
    }

    /// Updates the pill (window size/position/visibility) and notifies its webview.
    pub fn set_overlay(&self, mode: OverlayMode, live_text: Option<String>) {
        let state = {
            let mut o = self.overlay.lock().unwrap();
            if mode != OverlayMode::Listening {
                o.live_text.clear();
            }
            if o.mode != mode {
                o.mode = mode;
            }
            if let Some(t) = live_text {
                o.live_text = t;
            }
            o.clone()
        };
        if let Some(app) = self.app() {
            crate::windows::apply_overlay(&app, &state);
            let _ = app.emit("overlay", state);
        }
    }
}
