//! The app's brain (AppDelegate.swift): owns the dictation state machine, engines and meetings.
//! Everything else (hotkey, tray, commands, pill buttons) talks to it through `Msg`.

use jez_core::model::{AppInfo, InputDevice};
use jez_core::traits::HotkeyEvent;
use jez_core::Config;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

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

/// Placeholder loop until the engines and platform layers land (phase 2).
pub fn spawn(
    app: tauri::AppHandle,
    _shared: crate::state::SharedRef,
    _handle: Handle,
    mut rx: mpsc::UnboundedReceiver<Msg>,
) {
    tauri::async_runtime::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Msg::Quit = msg {
                app.exit(0);
            }
        }
    });
}
