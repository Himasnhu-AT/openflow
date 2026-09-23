//! Meeting mic watchdog (MeetingSession.startWatchdog): if the microphone delivers digital silence for 4 s
//! (built-in mic with the lid closed, AirPods gone to the phone), switch to another input rather than record
//! nothing.
//!
//! The Swift version read the peak of the last 16000 samples of `mic.snapshot()` every second. Copying the whole
//! meeting's buffer each second gets expensive in long meetings, so here the same window is kept incrementally:
//! the session must call [`MicWatchdog::feed`] with every mic chunk (from the mic's `on_chunk` sink). A mic that
//! delivers nothing at all keeps the last window, exactly as the snapshot would.

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use jez_core::traits::{AudioDevices, Mic};
use tokio::task::JoinHandle;
use tokio::time::Instant;

/// Samples considered (1 s at 16 kHz).
const WINDOW: usize = 16_000;
/// Anything above digital silence counts as a live mic.
const SILENCE_PEAK: f32 = 1e-5;
const SILENT_FOR: Duration = Duration::from_secs(4);

struct Shared {
    window: VecDeque<f32>,
    preference: String,
}

pub struct MicWatchdog {
    shared: Arc<Mutex<Shared>>,
    task: JoinHandle<()>,
}

impl MicWatchdog {
    /// Starts watching (checks every second). `preference` is the device the mic was started with
    /// ("" = default, "builtin", or a UID). Must be called from within a tokio runtime.
    pub fn spawn(mic: Arc<dyn Mic>, devices: Arc<dyn AudioDevices>, preference: &str) -> MicWatchdog {
        let shared = Arc::new(Mutex::new(Shared { window: VecDeque::with_capacity(WINDOW), preference: preference.to_string() }));
        let s2 = Arc::clone(&shared);
        let task = tokio::spawn(async move {
            let mut silent_since: Option<Instant> = None;
            let mut tried: HashSet<String> = HashSet::new();
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let (peak, pref) = {
                    let s = s2.lock().unwrap();
                    (s.window.iter().fold(0f32, |m, x| m.max(x.abs())), s.preference.clone())
                };
                if peak > SILENCE_PEAK {
                    silent_since = None;
                    continue;
                }
                let now = Instant::now();
                let since = *silent_since.get_or_insert(now);
                if now.duration_since(since) < SILENT_FOR {
                    continue;
                }
                silent_since = None;
                let (mic, devices) = (Arc::clone(&mic), Arc::clone(&devices));
                let tried_now = tried.clone();
                // Device enumeration and restarting the mic touch Core Audio; keep them off the async workers.
                let switched = tokio::task::spawn_blocking(move || switch_device(&*mic, &*devices, &pref, tried_now))
                    .await
                    .unwrap_or((None, None));
                if let Some(uid) = switched.0 {
                    tried.insert(uid);
                }
                if let Some(next) = switched.1 {
                    let mut s = s2.lock().unwrap();
                    s.preference = next;
                    s.window.clear(); // the restarted mic's buffer starts empty
                }
            }
        });
        MicWatchdog { shared, task }
    }

    /// Feed every mic chunk (16 kHz mono) here.
    pub fn feed(&self, chunk: &[f32]) {
        Self::feed_window(&mut self.shared.lock().unwrap().window, chunk);
    }

    /// A cloneable feeder for use inside a `ChunkSink`.
    pub fn feeder(&self) -> impl Fn(&[f32]) + Send + Sync + 'static {
        let shared = Arc::clone(&self.shared);
        move |c: &[f32]| Self::feed_window(&mut shared.lock().unwrap().window, c)
    }

    /// The device preference currently in use (changes when the watchdog switched devices).
    pub fn preference(&self) -> String {
        self.shared.lock().unwrap().preference.clone()
    }

    pub fn stop(&self) {
        self.task.abort();
    }
}

impl Drop for MicWatchdog {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl MicWatchdog {
    fn feed_window(w: &mut VecDeque<f32>, chunk: &[f32]) {
        if chunk.len() >= WINDOW {
            w.clear();
            w.extend(&chunk[chunk.len() - WINDOW..]);
            return;
        }
        let overflow = (w.len() + chunk.len()).saturating_sub(WINDOW);
        w.drain(..overflow);
        w.extend(chunk);
    }
}

/// Returns (current device uid to mark tried, new preference if switched).
fn switch_device(
    mic: &dyn Mic,
    devices: &dyn AudioDevices,
    preference: &str,
    mut tried: HashSet<String>,
) -> (Option<String>, Option<String>) {
    let current = devices.resolve(preference);
    let current_uid = current.as_ref().map(|c| c.uid.clone());
    if let Some(uid) = &current_uid {
        tried.insert(uid.clone());
    }
    let lid_closed = devices.lid_closed();
    let Some(other) = devices
        .inputs()
        .into_iter()
        .find(|d| !tried.contains(&d.uid) && !d.is_continuity && !(d.is_built_in && lid_closed))
    else {
        return (current_uid, None);
    };
    tracing::error!(target: "meeting",
        "Microphone silent for 4 s on {}; switching to {}",
        current.map(|c| c.name).unwrap_or_else(|| "?".into()), other.name);
    let _ = mic.stop();
    if let Err(e) = mic.start(&other.uid) {
        tracing::error!(target: "meeting", "Mic restart on {} failed: {e:#}", other.name);
    }
    (current_uid, Some(other.uid))
}

#[cfg(test)]
mod tests {
    use super::*;
    use jez_core::model::InputDevice;
    use jez_core::traits::{ChunkSink, LevelSink};
    use std::sync::atomic::{AtomicBool, Ordering};

    #[derive(Default)]
    struct FakeMic {
        starts: Mutex<Vec<String>>,
        stops: Mutex<usize>,
    }
    impl Mic for FakeMic {
        fn start(&self, preferred_device: &str) -> anyhow::Result<()> {
            self.starts.lock().unwrap().push(preferred_device.to_string());
            Ok(())
        }
        fn snapshot(&self) -> Vec<f32> {
            vec![]
        }
        fn stop(&self) -> Vec<f32> {
            *self.stops.lock().unwrap() += 1;
            vec![]
        }
        fn shutdown(&self) {}
        fn is_running(&self) -> bool {
            true
        }
        fn set_on_level(&self, _: Option<LevelSink>) {}
        fn set_on_chunk(&self, _: Option<ChunkSink>) {}
        fn current_device_uid(&self) -> Option<String> {
            None
        }
    }

    struct Devs {
        list: Vec<InputDevice>,
        lid: AtomicBool,
    }
    impl AudioDevices for Devs {
        fn inputs(&self) -> Vec<InputDevice> {
            self.list.clone()
        }
        fn resolve(&self, preference: &str) -> Option<InputDevice> {
            match preference {
                "" | "builtin" => self.list.iter().find(|d| d.is_built_in).cloned(),
                uid => self.list.iter().find(|d| d.uid == uid).cloned(),
            }
        }
        fn lid_closed(&self) -> bool {
            self.lid.load(Ordering::SeqCst)
        }
        fn microphone_in_use(&self) -> bool {
            false
        }
    }

    fn dev(uid: &str, built_in: bool, continuity: bool) -> InputDevice {
        InputDevice { uid: uid.into(), name: uid.into(), is_built_in: built_in, is_continuity: continuity }
    }

    async fn settle() {
        for _ in 0..20 {
            tokio::task::yield_now().await;
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    async fn advance_secs(n: u64) {
        for _ in 0..n {
            tokio::time::advance(Duration::from_secs(1)).await;
            settle().await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn switches_after_four_seconds_of_silence() {
        let mic = Arc::new(FakeMic::default());
        let devs = Arc::new(Devs {
            list: vec![dev("iphone", false, true), dev("builtin", true, false), dev("usb", false, false)],
            lid: AtomicBool::new(false),
        });
        let wd = MicWatchdog::spawn(mic.clone(), devs.clone(), "");
        settle().await;
        // Silent from t=1 (first check), switch at t=5.
        advance_secs(4).await;
        assert!(mic.starts.lock().unwrap().is_empty());
        advance_secs(1).await;
        assert_eq!(*mic.starts.lock().unwrap(), vec!["usb"]);
        assert_eq!(*mic.stops.lock().unwrap(), 1);
        assert_eq!(wd.preference(), "usb");
        // Still silent on usb: builtin is tried, iphone is continuity, so nothing is left.
        advance_secs(10).await;
        assert_eq!(*mic.starts.lock().unwrap(), vec!["usb"]);
    }

    #[tokio::test(start_paused = true)]
    async fn audio_resets_the_clock() {
        let mic = Arc::new(FakeMic::default());
        let devs = Arc::new(Devs { list: vec![dev("a", true, false), dev("b", false, false)], lid: AtomicBool::new(false) });
        let wd = MicWatchdog::spawn(mic.clone(), devs, "a");
        let feed = wd.feeder();
        settle().await;
        advance_secs(3).await;
        feed(&[0.0, 0.01, 0.0]);
        advance_secs(3).await;
        // Signal still within the last 16000 samples.
        assert!(mic.starts.lock().unwrap().is_empty());
        wd.feed(&vec![0.0; 16_000]);
        advance_secs(4).await;
        assert!(mic.starts.lock().unwrap().is_empty());
        advance_secs(1).await;
        assert_eq!(*mic.starts.lock().unwrap(), vec!["b"]);
    }

    #[tokio::test(start_paused = true)]
    async fn skips_builtin_when_lid_closed() {
        let mic = Arc::new(FakeMic::default());
        let devs = Arc::new(Devs {
            list: vec![dev("usb", false, false), dev("builtin", true, false)],
            lid: AtomicBool::new(true),
        });
        let _wd = MicWatchdog::spawn(mic.clone(), devs, "usb");
        settle().await;
        advance_secs(20).await;
        assert!(mic.starts.lock().unwrap().is_empty());
    }

    #[test]
    fn window_keeps_last_second() {
        let mut w = VecDeque::new();
        MicWatchdog::feed_window(&mut w, &[1.0; 10]);
        MicWatchdog::feed_window(&mut w, &vec![0.0; 15_995]);
        assert_eq!(w.len(), WINDOW);
        assert_eq!(w.iter().filter(|x| **x == 1.0).count(), 5);
        MicWatchdog::feed_window(&mut w, &vec![0.5; 20_000]);
        assert_eq!(w.len(), WINDOW);
        assert!(w.iter().all(|x| *x == 0.5));
    }
}
