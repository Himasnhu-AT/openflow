//! One live meeting (MeetingSession.swift): microphone ("You") and system audio ("Them") are split into
//! utterances, transcribed one at a time as they end, and appended to the meeting in the store.
//! Nothing is written to disk but the transcript.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use jez_core::model::MeetingSegment;
use jez_core::segmenter::Segmenter;
use jez_core::text_rules;
use jez_core::traits::{AudioDevices, Mic, SystemAudio};
use jez_engines::meeting_engine::MeetingEngine;
use jez_platform::mic_watchdog::MicWatchdog;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::state::SharedRef;

struct Utterance {
    samples: Vec<f32>,
    start: f64,
    speaker: &'static str,
}

pub struct MeetingSession {
    pub meeting_id: Uuid,
    shared: SharedRef,
    engine: Arc<MeetingEngine>,
    mic: Arc<dyn Mic>,
    devices: Arc<dyn AudioDevices>,
    system: Arc<dyn SystemAudio>,
    mic_segmenter: Arc<Segmenter>,
    tap_segmenter: Arc<Segmenter>,
    tx: Mutex<Option<mpsc::UnboundedSender<Utterance>>>,
    rx: Mutex<Option<mpsc::UnboundedReceiver<Utterance>>>,
    watchdog: Mutex<Option<MicWatchdog>>,
    stopped: AtomicBool,
    pub system_audio_working: AtomicBool,
}

impl MeetingSession {
    pub fn new(
        meeting_id: Uuid,
        shared: SharedRef,
        engine: Arc<MeetingEngine>,
        mic: Arc<dyn Mic>,
        devices: Arc<dyn AudioDevices>,
        system: Arc<dyn SystemAudio>,
    ) -> Arc<MeetingSession> {
        let (tx, rx) = mpsc::unbounded_channel::<Utterance>();
        let mic_tx = tx.clone();
        let tap_tx = tx.clone();
        let mic_segmenter = Segmenter::new(Box::new(move |samples, start| {
            let _ = mic_tx.send(Utterance { samples, start, speaker: "You" });
        }));
        let mut tap_segmenter = Segmenter::new(Box::new(move |samples, start| {
            let _ = tap_tx.send(Utterance { samples, start, speaker: "Them" });
        }));
        tap_segmenter.speech_floor = 0.010; // line noise and comfort noise must not count as speech
        tap_segmenter.min_speech_frames = 15; // 300 ms
        Arc::new(MeetingSession {
            meeting_id,
            shared,
            engine,
            mic,
            devices,
            system,
            mic_segmenter: Arc::new(mic_segmenter),
            tap_segmenter: Arc::new(tap_segmenter),
            tx: Mutex::new(Some(tx)),
            rx: Mutex::new(Some(rx)),
            watchdog: Mutex::new(None),
            stopped: AtomicBool::new(false),
            system_audio_working: AtomicBool::new(true),
        })
    }

    /// `microphone` is the device preference; `tap_app` limits the tap to one app's audio ("" = everything).
    pub fn start(self: &Arc<Self>, microphone: &str, tap_app: &str) -> anyhow::Result<()> {
        let watchdog = MicWatchdog::spawn(Arc::clone(&self.mic), Arc::clone(&self.devices), microphone);
        let feed = watchdog.feeder();
        let seg = Arc::clone(&self.mic_segmenter);
        self.mic.set_on_level(None);
        self.mic.set_on_chunk(Some(Arc::new(move |c: &[f32]| {
            seg.push(c);
            feed(c);
        })));
        self.mic.start(microphone)?;
        *self.watchdog.lock().unwrap() = Some(watchdog);

        let tap = Arc::clone(&self.tap_segmenter);
        if let Err(e) = self.system.start(tap_app, Arc::new(move |c: &[f32]| tap.push(c))) {
            self.system_audio_working.store(false, Ordering::SeqCst);
            tracing::error!("system audio: {e:#}");
        }

        let rx = self.rx.lock().unwrap().take().expect("start once");
        let me = Arc::clone(self);
        tauri::async_runtime::spawn(async move { me.drain(rx).await });
        Ok(())
    }

    /// Idempotent. Remaining utterances drain; the store then marks the meeting finished.
    pub fn stop(&self) {
        if self.stopped.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Some(w) = self.watchdog.lock().unwrap().take() {
            w.stop();
        }
        self.mic.set_on_chunk(None);
        self.mic.stop();
        self.mic.shutdown();
        let system = Arc::clone(&self.system);
        // Destroying the aggregate device can block for a moment.
        std::thread::spawn(move || system.stop());
        self.mic_segmenter.finish();
        self.tap_segmenter.finish();
        // Dropping the last sender ends the drain loop once the queue is empty.
        self.tx.lock().unwrap().take();
    }

    /// Serial, so the two channels never compete for the model.
    async fn drain(self: Arc<Self>, mut rx: mpsc::UnboundedReceiver<Utterance>) {
        let _ = self.engine.load().await;
        let mut last_speaker = String::new();
        let mut last_end = -10.0f64;
        let mut last_segment: Option<Uuid> = None;
        loop {
            let next = tokio::select! {
                u = rx.recv() => u,
                // The segmenter callbacks hold sender clones, so also watch the stop flag.
                _ = tokio::time::sleep(Duration::from_millis(200)), if self.stopped.load(Ordering::SeqCst) => {
                    match rx.try_recv() { Ok(u) => Some(u), Err(_) => None }
                }
            };
            let Some(u) = next else { break };
            let (text, english) = match self.engine.transcribe(&u.samples, u.speaker).await {
                Ok(r) if !r.0.is_empty() => r,
                Ok(_) => continue,
                Err(e) => {
                    tracing::error!("meeting transcribe: {e:#}");
                    continue;
                }
            };
            let config = self.shared.config();
            let text = if english {
                text_rules::meeting_line(&text, &config.dictionary)
            } else {
                text_rules::chinese_script(&text_rules::apply_dictionary(&text, &config.dictionary), config.traditional_chinese())
            };
            if text.is_empty() {
                continue;
            }
            let end = u.start + u.samples.len() as f64 / jez_core::SAMPLE_RATE as f64;
            let mut merged = false;
            if u.speaker == last_speaker && u.start - last_end < 1.5 {
                if let Some(prev_id) = last_segment {
                    let mut store = self.shared.store();
                    let id = self.meeting_id;
                    store.update_meeting(id, |m| {
                        if let Some(prev) = m.segments.iter_mut().find(|s| s.id == prev_id) {
                            if prev.text.chars().count() + text.chars().count() < 500 {
                                prev.text = format!("{} {}", prev.text, text);
                                merged = true;
                            }
                        }
                    });
                }
            }
            if !merged {
                let seg = MeetingSegment { id: Uuid::new_v4(), time: u.start, speaker: u.speaker.to_string(), text };
                last_segment = Some(seg.id);
                self.shared.store().append_segment(self.meeting_id, seg);
            }
            last_speaker = u.speaker.to_string();
            last_end = end;
            self.shared.emit_snapshot();
        }
        self.shared.store().finish_meeting(self.meeting_id);
        self.shared.emit_snapshot();
    }
}
