//! OWNER: team E (platform). Call detection loop (MeetingSession.swift MeetingDetector), written against the
//! Apps/AudioDevices traits.
//!
//! Polls for a call in progress: a meeting app running while the microphone is open, or a browser tab whose
//! title looks like a call. Fires once per call; resets when the call ends.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use openflow_core::model::AppInfo;
use openflow_core::traits::{Apps, AudioDevices, RecordingProbe};
use tokio::task::JoinHandle;
use tokio::time::Instant;

/// Bundle id → display name of apps that host calls.
pub const CALL_APPS: &[(&str, &str)] = &[
    ("us.zoom.xos", "Zoom"),
    ("com.microsoft.teams2", "Teams"),
    ("com.microsoft.teams", "Teams"),
    ("com.apple.FaceTime", "FaceTime"),
    ("com.tinyspeck.slackmacgap", "Slack"),
    ("com.hnc.Discord", "Discord"),
    ("com.skype.skype", "Skype"),
    ("com.cisco.webexmeetingsapp", "Webex"),
    ("com.webex.meetingmanager", "Webex"),
    ("com.loom.desktop", "Loom"),
    ("com.ringcentral.glip", "RingCentral"),
    ("com.logmein.GoToMeeting", "GoTo"),
    ("com.gotomeeting.GoToMeeting", "GoTo"),
    ("com.around.Around", "Around"),
    ("com.electron.lark", "Lark"),
    ("com.tencent.meeting", "Tencent Meeting"),
    ("com.zhumu.zhumu", "Zhumu"),
    ("com.bluejeans.app", "BlueJeans"),
    ("com.dialpad.desktop", "Dialpad"),
    ("com.riverside.desktop", "Riverside"),
    ("com.jitsi.jitsi-meet", "Jitsi"),
    ("com.whereby.desktop", "Whereby"),
    ("com.streamyard.desktop", "StreamYard"),
    ("com.pop.desktop", "Pop"),
    ("com.readdle.spark", "Spark"),
    ("com.google.meet", "Google Meet"),
    ("com.linphone.linphone", "Linphone"),
    ("com.facebook.archon", "Messenger"),
    ("com.facebook.archon.developerID", "Messenger"),
    ("com.apple.MobileSMS", "Messages"),
    ("net.whatsapp.WhatsApp", "WhatsApp"),
    ("com.viber.osx", "Viber"),
    ("org.telegram.desktop", "Telegram"),
    ("ru.keepcoder.Telegram", "Telegram"),
    ("com.signal.desktop", "Signal"),
    ("org.whispersystems.signal-desktop", "Signal"),
    ("com.tencent.xinWeChat", "WeChat"),
    ("jp.naver.line.mac", "LINE"),
];

pub const BROWSERS: &[&str] = &[
    "com.google.Chrome",
    "com.apple.Safari",
    "company.thebrowser.Browser",
    "org.mozilla.firefox",
    "com.microsoft.edgemac",
    "com.brave.Browser",
    "com.vivaldi.Vivaldi",
    "com.operasoftware.Opera",
    "com.google.Chrome.canary",
    "org.chromium.Chromium",
    "com.apple.Safari.WebApp",
];

/// Substrings of a lowercased browser window title that mean a call tab.
pub const TITLE_HINTS: &[&str] = &[
    "messenger",
    "facebook.com/call",
    "meet.google",
    "google meet",
    "meet -",
    "zoom",
    "teams",
    "webex",
    "whereby",
    "around.co",
    "huddle",
    "jitsi",
    "discord",
    "goto",
    "dialpad",
    "riverside",
    "streamyard",
    "zoho meeting",
    "bluejeans",
    "skype",
    "call -",
    "video call",
    "meeting",
];

const TICK: Duration = Duration::from_secs(3);
const IDLE_END: Duration = Duration::from_secs(20);
/// Unknown app holding the mic for this many ticks (15 s): offer anyway, naming the frontmost app.
const BUSY_FALLBACK_TICKS: u32 = 5;

pub type FrontmostApp = Arc<dyn Fn() -> Option<AppInfo> + Send + Sync>;
pub type DetectedSink = Arc<dyn Fn(String) + Send + Sync>;
pub type EndedSink = Arc<dyn Fn() + Send + Sync>;

pub fn call_app_name(bundle_id: &str) -> Option<&'static str> {
    CALL_APPS.iter().find(|(id, _)| *id == bundle_id).map(|(_, n)| *n)
}

#[derive(Default)]
struct State {
    announced: bool,
    idle_since: Option<Instant>,
    busy_ticks: u32,
    current_call_bundle: String,
}

#[derive(Default)]
struct Callbacks {
    is_recording: Option<RecordingProbe>,
    on_detected: Option<DetectedSink>,
    on_ended: Option<EndedSink>,
}

struct Inner {
    apps: Arc<dyn Apps>,
    devices: Arc<dyn AudioDevices>,
    frontmost: FrontmostApp,
    state: Mutex<State>,
    callbacks: Mutex<Callbacks>,
    task: Mutex<Option<JoinHandle<()>>>,
}

#[derive(Clone)]
pub struct MeetingDetector {
    inner: Arc<Inner>,
}

impl MeetingDetector {
    pub fn new(apps: Arc<dyn Apps>, devices: Arc<dyn AudioDevices>, frontmost: FrontmostApp) -> Self {
        MeetingDetector {
            inner: Arc::new(Inner {
                apps,
                devices,
                frontmost,
                state: Mutex::default(),
                callbacks: Mutex::default(),
                task: Mutex::new(None),
            }),
        }
    }

    /// Our own capture keeps the mic open: while this returns true nothing is detected.
    pub fn set_is_recording(&self, probe: RecordingProbe) {
        self.inner.callbacks.lock().unwrap().is_recording = Some(probe);
    }

    pub fn set_on_detected(&self, sink: DetectedSink) {
        self.inner.callbacks.lock().unwrap().on_detected = Some(sink);
    }

    pub fn set_on_ended(&self, sink: EndedSink) {
        self.inner.callbacks.lock().unwrap().on_ended = Some(sink);
    }

    /// Starts polling every 3 s. Must be called from within a tokio runtime. No-op when already running.
    pub fn start(&self) {
        let mut task = self.inner.task.lock().unwrap();
        if task.is_some() {
            return;
        }
        let inner = Arc::clone(&self.inner);
        *task = Some(tokio::spawn(async move {
            let mut interval = tokio::time::interval_at(Instant::now() + TICK, TICK);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                let inner = Arc::clone(&inner);
                // OS queries (Accessibility window titles, Core Audio) can block; keep them off the async workers.
                let _ = tokio::task::spawn_blocking(move || inner.tick()).await;
            }
        }));
    }

    pub fn stop(&self) {
        if let Some(t) = self.inner.task.lock().unwrap().take() {
            t.abort();
        }
    }

    /// One poll (normally driven by `start`).
    pub fn tick(&self) {
        self.inner.tick()
    }

    /// Name of the call in progress, if any. Updates `current_call_bundle`.
    pub fn current_call(&self) -> Option<String> {
        self.inner.current_call()
    }

    /// Bundle id of the app the last detected call runs in (used to tap only its audio).
    pub fn current_call_bundle(&self) -> String {
        self.inner.state.lock().unwrap().current_call_bundle.clone()
    }

    /// Called when a meeting is stopped or declined so the same call isn't offered again.
    pub fn mark_handled(&self) {
        self.inner.state.lock().unwrap().announced = true;
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(t) = self.task.get_mut().unwrap().take() {
            t.abort();
        }
    }
}

impl Inner {
    fn current_call(&self) -> Option<String> {
        let running = self.apps.running();
        let mut found: Option<(String, String)> = None;
        for app in &running {
            if let Some(name) = call_app_name(&app.bundle_id) {
                found = Some((app.bundle_id.clone(), name.to_string()));
                break;
            }
        }
        if found.is_none() {
            'browsers: for app in running.iter().filter(|a| BROWSERS.contains(&a.bundle_id.as_str())) {
                for title in self.apps.window_titles(&app.bundle_id) {
                    let t = title.to_lowercase();
                    if TITLE_HINTS.iter().any(|h| t.contains(h)) {
                        let name = if app.name.is_empty() { "browser".to_string() } else { app.name.clone() };
                        found = Some((app.bundle_id.clone(), name));
                        break 'browsers;
                    }
                }
            }
        }
        let (bundle, name) = match found {
            Some((b, n)) => (b, Some(n)),
            None => ((self.frontmost)().map(|a| a.bundle_id).unwrap_or_default(), None),
        };
        self.state.lock().unwrap().current_call_bundle = bundle;
        name
    }

    fn tick(&self) {
        let (is_recording, on_detected, on_ended) = {
            let cb = self.callbacks.lock().unwrap();
            (cb.is_recording.clone(), cb.on_detected.clone(), cb.on_ended.clone())
        };
        let mic_busy = self.devices.microphone_in_use();
        if is_recording.is_some_and(|p| p()) {
            let mut s = self.state.lock().unwrap();
            s.idle_since = None;
            s.busy_ticks = 0;
            return;
        }
        let now = Instant::now();
        if !mic_busy {
            let ended = {
                let mut s = self.state.lock().unwrap();
                s.busy_ticks = 0;
                if s.announced {
                    let since = *s.idle_since.get_or_insert(now);
                    if now.duration_since(since) > IDLE_END {
                        s.announced = false;
                        s.idle_since = None;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            };
            if ended {
                tracing::info!(target: "meeting", "Call ended");
                if let Some(f) = on_ended {
                    f();
                }
            }
            return;
        }
        let busy_ticks = {
            let mut s = self.state.lock().unwrap();
            s.idle_since = None;
            s.busy_ticks += 1;
            s.busy_ticks
        };
        let mut call = self.current_call();
        if call.is_none() && busy_ticks >= BUSY_FALLBACK_TICKS {
            if let Some(front) = (self.frontmost)() {
                if !front.name.is_empty() && front.name != "OpenFlow" {
                    call = Some(front.name);
                }
            }
        }
        let Some(call) = call else { return };
        let fire = {
            let mut s = self.state.lock().unwrap();
            !std::mem::replace(&mut s.announced, true)
        };
        if fire {
            tracing::info!(target: "meeting", "Call detected: {call}");
            if let Some(f) = on_detected {
                f(call);
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use openflow_core::model::InputDevice;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[derive(Default)]
    pub struct FakeApps {
        pub running: Mutex<Vec<AppInfo>>,
        pub titles: Mutex<Vec<(String, String)>>,
    }
    impl Apps for FakeApps {
        fn running(&self) -> Vec<AppInfo> {
            self.running.lock().unwrap().clone()
        }
        fn info(&self, bundle_id: &str) -> Option<AppInfo> {
            self.running().into_iter().find(|a| a.bundle_id == bundle_id)
        }
        fn window_titles(&self, bundle_id: &str) -> Vec<String> {
            self.titles.lock().unwrap().iter().filter(|(b, _)| b == bundle_id).map(|(_, t)| t.clone()).collect()
        }
    }

    #[derive(Default)]
    pub struct FakeDevices {
        pub busy: AtomicBool,
    }
    impl AudioDevices for FakeDevices {
        fn inputs(&self) -> Vec<InputDevice> {
            vec![]
        }
        fn resolve(&self, _: &str) -> Option<InputDevice> {
            None
        }
        fn lid_closed(&self) -> bool {
            false
        }
        fn microphone_in_use(&self) -> bool {
            self.busy.load(Ordering::SeqCst)
        }
    }

    fn app(b: &str, n: &str) -> AppInfo {
        AppInfo { bundle_id: b.into(), name: n.into(), icon: None }
    }

    struct Rig {
        det: MeetingDetector,
        apps: Arc<FakeApps>,
        devices: Arc<FakeDevices>,
        front: Arc<Mutex<Option<AppInfo>>>,
        recording: Arc<AtomicBool>,
        events: Arc<Mutex<Vec<String>>>,
    }

    fn rig() -> Rig {
        let apps = Arc::new(FakeApps::default());
        let devices = Arc::new(FakeDevices::default());
        let front = Arc::new(Mutex::new(Some(app("com.apple.Terminal", "Terminal"))));
        let f2 = Arc::clone(&front);
        let det = MeetingDetector::new(apps.clone(), devices.clone(), Arc::new(move || f2.lock().unwrap().clone()));
        let recording = Arc::new(AtomicBool::new(false));
        let r2 = Arc::clone(&recording);
        det.set_is_recording(Arc::new(move || r2.load(Ordering::SeqCst)));
        let events = Arc::new(Mutex::new(Vec::new()));
        let (e1, e2) = (Arc::clone(&events), Arc::clone(&events));
        det.set_on_detected(Arc::new(move |c| e1.lock().unwrap().push(format!("detected {c}"))));
        det.set_on_ended(Arc::new(move || e2.lock().unwrap().push("ended".into())));
        Rig { det, apps, devices, front, recording, events }
    }

    impl Rig {
        fn busy(&self, b: bool) {
            self.devices.busy.store(b, Ordering::SeqCst);
        }
        fn events(&self) -> Vec<String> {
            self.events.lock().unwrap().clone()
        }
        async fn ticks(&self, n: usize) {
            for _ in 0..n {
                tokio::time::advance(TICK).await;
                self.det.tick();
            }
        }
    }

    #[test]
    fn tables() {
        assert_eq!(CALL_APPS.len(), 38);
        assert_eq!(BROWSERS.len(), 11);
        assert_eq!(TITLE_HINTS.len(), 23);
        assert_eq!(call_app_name("com.microsoft.teams2"), Some("Teams"));
    }

    #[tokio::test(start_paused = true)]
    async fn call_app_with_mic_busy_fires_once() {
        let r = rig();
        r.apps.running.lock().unwrap().extend([app("com.apple.Safari", "Safari"), app("us.zoom.xos", "zoom.us")]);
        r.ticks(1).await;
        assert!(r.events().is_empty()); // mic idle
        r.busy(true);
        r.ticks(3).await;
        assert_eq!(r.events(), vec!["detected Zoom"]);
        assert_eq!(r.det.current_call_bundle(), "us.zoom.xos");
    }

    #[tokio::test(start_paused = true)]
    async fn browser_title_hint() {
        let r = rig();
        r.apps.running.lock().unwrap().push(app("com.google.Chrome", "Google Chrome"));
        r.apps.titles.lock().unwrap().push(("com.google.Chrome".into(), "Inbox - Gmail".into()));
        assert_eq!(r.det.current_call(), None);
        assert_eq!(r.det.current_call_bundle(), "com.apple.Terminal");
        r.apps.titles.lock().unwrap().push(("com.google.Chrome".into(), "Meet - abc-defg-hij".into()));
        assert_eq!(r.det.current_call().as_deref(), Some("Google Chrome"));
        assert_eq!(r.det.current_call_bundle(), "com.google.Chrome");
        // Titles of non-browsers are never consulted.
        let r = rig();
        r.apps.running.lock().unwrap().push(app("com.apple.Notes", "Notes"));
        r.apps.titles.lock().unwrap().push(("com.apple.Notes".into(), "Zoom notes".into()));
        assert_eq!(r.det.current_call(), None);
    }

    #[tokio::test(start_paused = true)]
    async fn unknown_app_fallback_after_five_busy_ticks() {
        let r = rig();
        r.busy(true);
        r.ticks(4).await;
        assert!(r.events().is_empty());
        r.ticks(1).await;
        assert_eq!(r.events(), vec!["detected Terminal"]);
    }

    #[tokio::test(start_paused = true)]
    async fn fallback_never_names_openflow() {
        let r = rig();
        *r.front.lock().unwrap() = Some(app("com.himanshuat.openflow", "OpenFlow"));
        r.busy(true);
        r.ticks(10).await;
        assert!(r.events().is_empty());
        *r.front.lock().unwrap() = None;
        r.ticks(2).await;
        assert!(r.events().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn busy_ticks_reset_when_mic_idle() {
        let r = rig();
        r.busy(true);
        r.ticks(4).await;
        r.busy(false);
        r.ticks(1).await;
        r.busy(true);
        r.ticks(4).await;
        assert!(r.events().is_empty());
        r.ticks(1).await;
        assert_eq!(r.events().len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn own_recording_suppresses_detection() {
        let r = rig();
        r.apps.running.lock().unwrap().push(app("us.zoom.xos", "zoom.us"));
        r.busy(true);
        r.recording.store(true, Ordering::SeqCst);
        r.ticks(10).await;
        assert!(r.events().is_empty());
        r.recording.store(false, Ordering::SeqCst);
        r.ticks(1).await;
        assert_eq!(r.events(), vec!["detected Zoom"]);
    }

    #[tokio::test(start_paused = true)]
    async fn ends_after_more_than_20s_idle_and_can_fire_again() {
        let r = rig();
        r.apps.running.lock().unwrap().push(app("com.hnc.Discord", "Discord"));
        r.busy(true);
        r.ticks(1).await;
        r.busy(false);
        // First idle tick starts the clock; ends when > 20 s have passed: ticks at 0,3,...,21 s -> 8 ticks.
        r.ticks(7).await;
        assert_eq!(r.events(), vec!["detected Discord"]);
        r.ticks(1).await;
        assert_eq!(r.events(), vec!["detected Discord", "ended"]);
        r.busy(true);
        r.ticks(1).await;
        assert_eq!(r.events(), vec!["detected Discord", "ended", "detected Discord"]);
    }

    #[tokio::test(start_paused = true)]
    async fn brief_mic_blip_does_not_end() {
        let r = rig();
        r.apps.running.lock().unwrap().push(app("com.hnc.Discord", "Discord"));
        r.busy(true);
        r.ticks(1).await;
        r.busy(false);
        r.ticks(5).await;
        r.busy(true);
        r.ticks(1).await;
        r.busy(false);
        r.ticks(7).await;
        assert_eq!(r.events(), vec!["detected Discord"]);
    }

    #[tokio::test(start_paused = true)]
    async fn mark_handled_suppresses_offer() {
        let r = rig();
        r.apps.running.lock().unwrap().push(app("us.zoom.xos", "zoom.us"));
        r.det.mark_handled();
        r.busy(true);
        r.ticks(3).await;
        assert!(r.events().is_empty());
        r.busy(false);
        r.ticks(8).await;
        assert_eq!(r.events(), vec!["ended"]);
    }

    #[tokio::test(start_paused = true)]
    async fn start_polls_every_three_seconds() {
        let r = rig();
        r.apps.running.lock().unwrap().push(app("us.zoom.xos", "zoom.us"));
        r.busy(true);
        r.det.start();
        tokio::time::sleep(Duration::from_millis(2900)).await;
        assert!(r.events().is_empty());
        tokio::time::sleep(Duration::from_millis(200)).await;
        // spawn_blocking runs on a real thread: give it a moment.
        for _ in 0..50 {
            if !r.events().is_empty() {
                break;
            }
            tokio::task::yield_now().await;
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(r.events(), vec!["detected Zoom"]);
        r.det.stop();
    }
}
