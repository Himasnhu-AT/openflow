//! One live meeting (MeetingSession.swift): microphone ("You") and system audio ("Them") are split into
//! utterances, transcribed one at a time as they end, and appended to the meeting in the store.
//! Nothing is written to disk but the transcript.
//!
//! Both channels are placed on one wall clock (seconds since the session started), which survives mic restarts
//! and lets a mic line be compared with what the far side said at the same moment: without headphones the mic
//! hears the speakers, and those lines are dropped as echo (openflow_core::echo).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use openflow_core::echo::{is_echo, Heard};
use openflow_core::model::MeetingSegment;
use openflow_core::speakers::SpeakerTimeline;
use openflow_core::segmenter::Segmenter;
use openflow_core::text_rules;
use openflow_core::whisper_filter::collapse_repeats;
use openflow_core::traits::{AudioDevices, Mic, SystemAudio};
use openflow_engines::meeting_engine::MeetingEngine;
use openflow_platform::mic_watchdog::MicWatchdog;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::state::SharedRef;

struct Utterance {
    samples: Vec<f32>,
    /// Seconds since the session started (wall clock).
    start: f64,
    speaker: &'static str,
    /// When it was queued, to bound how long a mic line waits for the far side.
    queued: Instant,
}

impl Utterance {
    fn end(&self) -> f64 {
        self.start + self.samples.len() as f64 / openflow_core::SAMPLE_RATE as f64
    }
}

/// Far-side lines kept for the echo check.
const FAR_MEMORY: f64 = 90.0;
/// A mic line never waits longer than this for the far side's matching line.
const MAX_HOLD: Duration = Duration::from_secs(30);

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
    /// Set by stop() once both segmenters have flushed their last utterance into the queue.
    flushed: AtomicBool,
    pub system_audio_working: AtomicBool,
    /// Last time the tap delivered audio.
    last_tap_audio: Arc<Mutex<Option<Instant>>>,
    started: Instant,
    /// Who is talking on the far side, from the call app (Zoom Accessibility, the Meet extension).
    speakers: Mutex<SpeakerTimeline>,
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
        let started = Instant::now();
        // The utterance ended `behind` seconds of audio before the chunk being processed now.
        let place = move |samples: &[f32], behind: f64| {
            let len = samples.len() as f64 / openflow_core::SAMPLE_RATE as f64;
            (started.elapsed().as_secs_f64() - behind - len).max(0.0)
        };
        let mic_segmenter = Segmenter::new(Box::new(move |samples, _, behind| {
            let start = place(&samples, behind);
            let _ = mic_tx.send(Utterance { samples, start, speaker: "You", queued: Instant::now() });
        }));
        let mut tap_segmenter = Segmenter::new(Box::new(move |samples, _, behind| {
            let start = place(&samples, behind);
            let _ = tap_tx.send(Utterance { samples, start, speaker: "Them", queued: Instant::now() });
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
            flushed: AtomicBool::new(false),
            system_audio_working: AtomicBool::new(true),
            last_tap_audio: Arc::new(Mutex::new(None)),
            started,
            speakers: Mutex::new(SpeakerTimeline::default()),
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
        let last = Arc::clone(&self.last_tap_audio);
        if let Err(e) = self.system.start(
            tap_app,
            Arc::new(move |c: &[f32]| {
                *last.lock().unwrap() = Some(Instant::now());
                tap.push(c)
            }),
        ) {
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
        self.flushed.store(true, Ordering::SeqCst);
        // Dropping the last sender ends the drain loop once the queue is empty.
        self.tx.lock().unwrap().take();
    }

    /// Seconds since the session started.
    fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// Speaker evidence from the call app, stamped now. `active` None: nobody remote is talking.
    pub fn observe_speaker(&self, active: Option<&str>, self_name: Option<&str>, participants: &[String]) {
        let now = self.now();
        let mut t = self.speakers.lock().unwrap();
        if let Some(me) = self_name {
            t.set_self(me);
        }
        for p in participants {
            t.saw_participant(p);
        }
        t.observe(now, active);
    }

    /// A caption block from the call app (Meet's live captions): `name` is saying `text`.
    pub fn observe_caption(&self, name: &str, text: &str) {
        let now = self.now();
        self.speakers.lock().unwrap().caption(now, name, text);
    }

    /// A far-side line's speaker: the call app's name for it, or "Them".
    fn far_speaker(&self, u: &Utterance, text: &str) -> String {
        self.speakers.lock().unwrap().attribute(u.start, u.end(), text).unwrap_or_else(|| "Them".into())
    }

    /// A held mic line may be transcribed once the far side can no longer produce a line overlapping it: the tap
    /// is not mid-utterance, or its current utterance began after the mic line ended. Without a working tap
    /// there is nothing to wait for.
    fn far_side_caught_up(&self, you: &Utterance) -> bool {
        let tap_live = self.last_tap_audio.lock().unwrap().is_some_and(|t| t.elapsed() < Duration::from_secs(5));
        if !tap_live || you.queued.elapsed() > MAX_HOLD {
            return true;
        }
        match self.tap_segmenter.speech_started_ago() {
            None => true,
            Some(ago) => self.now() - ago > you.end() + 0.3,
        }
    }

    /// Serial, so the two channels never compete for the model. Far-side lines go first; each mic line waits until
    /// the far side has caught up with it, so the echo check sees everything that was said at that moment.
    async fn drain(self: Arc<Self>, mut rx: mpsc::UnboundedReceiver<Utterance>) {
        let _ = self.engine.load().await;
        let mut writer = Writer::default();
        let mut far: VecDeque<Heard> = VecDeque::new();
        let mut held: VecDeque<Utterance> = VecDeque::new();
        let mut closed = false;
        loop {
            // Checked before draining the queue: whatever the tap emitted before this moment is already queued.
            let flushed = self.flushed.load(Ordering::SeqCst);
            let release = closed || held.front().is_some_and(|u| self.far_side_caught_up(u));
            let mut incoming = Vec::new();
            while let Ok(u) = rx.try_recv() {
                incoming.push(u);
            }
            if incoming.is_empty() && !release {
                if flushed && held.is_empty() {
                    break;
                }
                // The segmenter callbacks hold sender clones, so also wake on a timer to watch the stop flag.
                tokio::select! {
                    u = rx.recv() => match u {
                        Some(u) => incoming.push(u),
                        None => closed = true,
                    },
                    _ = tokio::time::sleep(Duration::from_millis(250)) => {}
                }
                if closed && held.is_empty() {
                    break;
                }
            }
            for u in incoming {
                if u.speaker == "You" {
                    held.push_back(u);
                    continue;
                }
                if let Some(text) = self.transcribe(&u).await {
                    far.push_back(Heard { start: u.start, end: u.end(), text: text.clone() });
                    let speaker = self.far_speaker(&u, &text);
                    writer.add(&self, &u, &speaker, text);
                }
            }
            while far.front().is_some_and(|h| h.end < self.now() - FAR_MEMORY) {
                far.pop_front();
            }
            // Checked, then the queue: a far-side line emitted meanwhile sends us back around to transcribe it first.
            while held.front().is_some_and(|u| closed || self.far_side_caught_up(u)) && rx.is_empty() {
                let u = held.pop_front().unwrap();
                let Some(text) = self.transcribe(&u).await else { continue };
                if is_echo(&text, u.start, u.end(), far.make_contiguous()) {
                    tracing::info!(target: "meeting", "Dropped speaker echo on the mic: {text}");
                    continue;
                }
                writer.add(&self, &u, "You", text);
            }
        }
        // Only now: stopping the speech server earlier would send the queued far-side audio to Parakeet.
        self.engine.shutdown();
        self.shared.store().finish_meeting(self.meeting_id);
        self.shared.emit_snapshot();
    }
}

impl MeetingSession {
    /// The cleaned line, or None when there is nothing to keep. One retry: a helper restart or a busy model
    /// should not cost a sentence of the meeting.
    async fn transcribe(&self, u: &Utterance) -> Option<String> {
        let mut result = self.engine.transcribe(&u.samples, u.speaker).await;
        if let Err(e) = &result {
            tracing::warn!("meeting transcribe failed, retrying: {e:#}");
            tokio::time::sleep(Duration::from_millis(500)).await;
            result = self.engine.transcribe(&u.samples, u.speaker).await;
        }
        let (text, english) = match result {
            Ok(r) if !r.0.trim().is_empty() => r,
            Ok(_) => return None,
            Err(e) => {
                tracing::error!("meeting transcribe: {e:#}");
                return None;
            }
        };
        let text = collapse_repeats(&text);
        let config = self.shared.config();
        let text = if english {
            text_rules::meeting_line(&text, &config.dictionary)
        } else {
            text_rules::chinese_script(&text_rules::apply_dictionary(&text, &config.dictionary), config.traditional_chinese())
        };
        (!text.is_empty()).then_some(text)
    }
}

/// Appends lines to the stored meeting, joining a speaker's consecutive lines into one paragraph.
#[derive(Default)]
struct Writer {
    last_speaker: String,
    last_end: f64,
    last_segment: Option<Uuid>,
}

impl Writer {
    fn add(&mut self, session: &MeetingSession, u: &Utterance, speaker: &str, text: String) {
        let mut merged = false;
        if speaker == self.last_speaker && u.start - self.last_end < 1.5 && u.start >= self.last_end - 0.5 {
            if let Some(prev_id) = self.last_segment {
                session.shared.store().update_meeting(session.meeting_id, |m| {
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
            let seg = MeetingSegment { id: Uuid::new_v4(), time: u.start, speaker: speaker.to_string(), text };
            self.last_segment = Some(seg.id);
            session.shared.store().append_segment(session.meeting_id, seg);
        }
        self.last_speaker = speaker.to_string();
        self.last_end = u.end();
        session.shared.emit_snapshot();
    }
}
