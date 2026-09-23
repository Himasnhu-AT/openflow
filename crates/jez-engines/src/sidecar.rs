//! OWNER: team D (sidecar). Client for the Swift jezflow-helper: AsrEngine, SystemAudio and NameSource impls (see docs/ARCHITECTURE.md protocol).
//!
//! One [`Sidecar`] owns one helper process. Frames go out through a writer thread and come back through a reader
//! thread that matches responses to requests by id and dispatches pushes (`tapAudio`, `tapRestarted`, `log`).
//! Plain threads keep the client independent of whichever tokio runtime (if any) the caller runs on: async
//! callers await a `tokio::sync::oneshot`, sync callers (the `SystemAudio` trait) wait on a std channel.
//!
//! When the helper dies, every pending request fails with "jezflow-helper exited", and a restore thread spawns
//! a fresh helper and replays the last successful `load`, the vocabulary and a running tap. Crash loops are
//! bounded: after [`MAX_RESTARTS`] deaths in [`RESTART_WINDOW`] automatic restores stop, and the next request
//! spawns the helper on demand.

use std::collections::HashMap;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context};
use async_trait::async_trait;
use jez_core::traits::{AsrEngine, ChunkSink, NameSource, Result, SystemAudio};
use serde_json::{json, Map, Value};

const HELPER_NAME: &str = "jezflow-helper";
const MAX_RESTARTS: usize = 5;
const RESTART_WINDOW: Duration = Duration::from_secs(60);
/// `tapStart` can sit behind the "System Audio Recording" permission prompt.
const TAP_START_TIMEOUT: Duration = Duration::from_secs(60);
const TAP_STOP_TIMEOUT: Duration = Duration::from_secs(5);
/// Upper bound for one frame header (the helper never sends more than a few KB).
const MAX_HEADER: usize = 64 << 20;

// ---------------------------------------------------------------------------------------------- framing

/// One protocol frame: JSON header plus optional 16 kHz mono f32 payload (`"audio": n` in the header).
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub header: Value,
    pub audio: Vec<f32>,
}

/// u32 LE header length, the JSON header, then `audio.len() × 4` bytes of f32 LE. Sets `"audio"` when non-empty.
pub fn encode_frame(header: &Value, audio: &[f32]) -> Vec<u8> {
    let mut header = header.clone();
    if !audio.is_empty() {
        if let Value::Object(map) = &mut header {
            map.insert("audio".into(), json!(audio.len()));
        }
    }
    let json = serde_json::to_vec(&header).unwrap_or_else(|_| b"{}".to_vec());
    let mut out = Vec::with_capacity(4 + json.len() + audio.len() * 4);
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&json);
    for s in audio {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Next frame, `Ok(None)` at a clean EOF.
pub fn read_frame(r: &mut impl Read) -> std::io::Result<Option<Frame>> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len) as usize;
    if len == 0 || len > MAX_HEADER {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("bad header length {len}"),
        ));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    let header: Value = serde_json::from_slice(&buf)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let n = header.get("audio").and_then(Value::as_u64).unwrap_or(0) as usize;
    let mut audio = Vec::new();
    if n > 0 {
        let mut bytes = vec![0u8; n * 4];
        r.read_exact(&mut bytes)?;
        audio = bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
    }
    Ok(Some(Frame { header, audio }))
}

// ---------------------------------------------------------------------------------------------- helper path

/// Where the helper binary is: `$JEZFLOW_HELPER`, else next to the current executable (Tauri externalBin, with
/// or without the target-triple suffix), else the SwiftPM release build in the workspace (dev).
pub fn helper_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("JEZFLOW_HELPER").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        for name in [
            exe_name(HELPER_NAME),
            exe_name(&format!("{HELPER_NAME}-{}", target_triple())),
        ] {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    let dev = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../sidecar/jezflow-helper/.build/release")
        .join(exe_name(HELPER_NAME));
    dev.is_file().then_some(dev)
}

fn exe_name(base: &str) -> String {
    format!("{base}{}", std::env::consts::EXE_SUFFIX)
}

/// Rust target triple as Tauri names externalBins (`jezflow-helper-aarch64-apple-darwin`).
fn target_triple() -> String {
    let arch = std::env::consts::ARCH;
    match std::env::consts::OS {
        "macos" => format!("{arch}-apple-darwin"),
        "windows" => format!("{arch}-pc-windows-msvc"),
        "linux" => format!("{arch}-unknown-linux-gnu"),
        os => format!("{arch}-unknown-{os}"),
    }
}

// ---------------------------------------------------------------------------------------------- connection

type Reply = std::result::Result<Value, String>;
type Waiter = Box<dyn FnOnce(Reply) + Send>;

struct Conn {
    generation: u64,
    frames: mpsc::Sender<Vec<u8>>,
    child: Arc<Mutex<Child>>,
}

/// What a fresh helper needs to be put back into the state the app last asked for.
#[derive(Default)]
struct Restore {
    version: Option<String>,
    vocabulary: Vec<String>,
    tap_bundle: Option<String>,
    deaths: Vec<Instant>,
}

/// Shared connection to the helper. Wrap in an `Arc` and hand clones to [`SidecarAsr`], [`SidecarSystemAudio`]
/// and [`SidecarNames`].
pub struct Sidecar {
    path: Option<PathBuf>,
    me: Weak<Sidecar>,
    conn: Mutex<Option<Conn>>,
    generation: AtomicU64,
    next_id: AtomicU64,
    pending: Mutex<HashMap<u64, (u64, Waiter)>>,
    tap_sink: Mutex<Option<ChunkSink>>,
    restore: Mutex<Restore>,
    model_ready: AtomicBool,
    closed: AtomicBool,
}

impl Sidecar {
    /// Uses [`helper_path`]. The process starts on the first request.
    pub fn new() -> Arc<Self> {
        Self::with_path(helper_path())
    }

    pub fn with_path(path: Option<PathBuf>) -> Arc<Self> {
        Arc::new_cyclic(|me| Sidecar {
            path,
            me: me.clone(),
            conn: Mutex::new(None),
            generation: AtomicU64::new(0),
            next_id: AtomicU64::new(1),
            pending: Mutex::new(HashMap::new()),
            tap_sink: Mutex::new(None),
            restore: Mutex::new(Restore::default()),
            model_ready: AtomicBool::new(false),
            closed: AtomicBool::new(false),
        })
    }

    pub fn helper(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Async request. Resolves when the helper answers or dies.
    pub async fn request(&self, method: &str, params: Value, audio: &[f32]) -> Result<Value> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.send(
            method,
            params,
            audio,
            Box::new(move |r| {
                let _ = tx.send(r);
            }),
        )?;
        match rx.await {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(e)) => Err(anyhow!(e)),
            Err(_) => bail!("{HELPER_NAME} dropped the request"),
        }
    }

    /// Blocking request (for sync traits and the restore thread). Don't call from a current-thread runtime.
    pub fn request_blocking(
        &self,
        method: &str,
        params: Value,
        audio: &[f32],
        timeout: Duration,
    ) -> Result<Value> {
        let (tx, rx) = mpsc::channel();
        self.send(
            method,
            params,
            audio,
            Box::new(move |r| {
                let _ = tx.send(r);
            }),
        )?;
        match rx.recv_timeout(timeout) {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(e)) => Err(anyhow!(e)),
            Err(_) => bail!("{HELPER_NAME}: {method} timed out"),
        }
    }

    fn send(&self, method: &str, params: Value, audio: &[f32], waiter: Waiter) -> Result<()> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut header = Map::new();
        header.insert("id".into(), json!(id));
        header.insert("method".into(), json!(method));
        if !params.is_null() {
            header.insert("params".into(), params);
        }
        let frame = encode_frame(&Value::Object(header), audio);
        let (generation, frames) = self.connection()?;
        self.pending
            .lock()
            .unwrap()
            .insert(id, (generation, waiter));
        // `died` clears the connection before sweeping pending requests: if the connection is already gone, the
        // sweep may have missed this one, so fail it here.
        let alive = self.conn.lock().unwrap().as_ref().map(|c| c.generation) == Some(generation);
        if !alive || frames.send(frame).is_err() {
            if let Some((_, w)) = self.pending.lock().unwrap().remove(&id) {
                w(Err(format!("{HELPER_NAME} exited")));
            }
        }
        Ok(())
    }

    /// The live connection, spawning the helper if there is none.
    fn connection(&self) -> Result<(u64, mpsc::Sender<Vec<u8>>)> {
        if self.closed.load(Ordering::SeqCst) {
            bail!("{HELPER_NAME} is shut down");
        }
        let mut conn = self.conn.lock().unwrap();
        if let Some(c) = conn.as_ref() {
            return Ok((c.generation, c.frames.clone()));
        }
        let c = self.spawn()?;
        let out = (c.generation, c.frames.clone());
        *conn = Some(c);
        Ok(out)
    }

    fn spawn(&self) -> Result<Conn> {
        let path = self.path.as_ref().ok_or_else(|| {
            anyhow!("{HELPER_NAME} not found (build sidecar/jezflow-helper or set JEZFLOW_HELPER)")
        })?;
        let mut child = Command::new(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| format!("starting {}", path.display()))?;
        let stdin = child.stdin.take().context("helper stdin")?;
        let stdout = child.stdout.take().context("helper stdout")?;
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let child = Arc::new(Mutex::new(child));
        thread::Builder::new()
            .name("jezflow-helper-writer".into())
            .spawn(move || writer(stdin, rx))?;
        let me = self.me.clone();
        let reader_child = child.clone();
        thread::Builder::new()
            .name("jezflow-helper-reader".into())
            .spawn(move || reader(me, stdout, generation, reader_child))?;
        tracing::info!(helper = %path.display(), generation, "jezflow-helper started");
        Ok(Conn {
            generation,
            frames: tx,
            child,
        })
    }

    fn dispatch(&self, frame: Frame) {
        let h = &frame.header;
        if let Some(id) = h.get("id").and_then(Value::as_u64) {
            let Some((_, waiter)) = self.pending.lock().unwrap().remove(&id) else {
                return;
            };
            let reply = match h.get("error") {
                Some(e) => Err(e
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| e.to_string())),
                None => Ok(h.get("result").cloned().unwrap_or(Value::Null)),
            };
            waiter(reply);
            return;
        }
        match h.get("event").and_then(Value::as_str).unwrap_or("") {
            "tapAudio" => {
                let sink = self.tap_sink.lock().unwrap().clone();
                if let Some(sink) = sink {
                    sink(&frame.audio);
                }
            }
            "tapRestarted" => {
                tracing::info!("system audio tap rebuilt after an output device change")
            }
            "log" => {
                let msg = h.get("message").and_then(Value::as_str).unwrap_or("");
                match h.get("level").and_then(Value::as_str).unwrap_or("info") {
                    "error" => tracing::error!(target: "jezflow_helper", "{msg}"),
                    "warn" | "warning" => tracing::warn!(target: "jezflow_helper", "{msg}"),
                    "debug" => tracing::debug!(target: "jezflow_helper", "{msg}"),
                    _ => tracing::info!(target: "jezflow_helper", "{msg}"),
                }
            }
            other => tracing::debug!(event = other, "unknown helper push"),
        }
    }

    /// Reader saw EOF (or garbage) on connection `generation`.
    fn died(&self, generation: u64) {
        {
            let mut conn = self.conn.lock().unwrap();
            if conn.as_ref().map(|c| c.generation) == Some(generation) {
                *conn = None;
            }
        }
        let failed: Vec<Waiter> = {
            let mut pending = self.pending.lock().unwrap();
            let ids: Vec<u64> = pending
                .iter()
                .filter(|(_, (g, _))| *g == generation)
                .map(|(id, _)| *id)
                .collect();
            ids.into_iter()
                .filter_map(|id| pending.remove(&id))
                .map(|(_, w)| w)
                .collect()
        };
        for w in failed {
            w(Err(format!("{HELPER_NAME} exited")));
        }
        self.model_ready.store(false, Ordering::SeqCst);
        if self.closed.load(Ordering::SeqCst) {
            return;
        }
        let restore = {
            let mut r = self.restore.lock().unwrap();
            let now = Instant::now();
            r.deaths.retain(|t| now.duration_since(*t) < RESTART_WINDOW);
            r.deaths.push(now);
            if r.deaths.len() > MAX_RESTARTS {
                tracing::error!("{HELPER_NAME} keeps exiting; not restarting it automatically");
                return;
            }
            r.version.is_some() || !r.vocabulary.is_empty() || r.tap_bundle.is_some()
        };
        tracing::warn!(generation, "{HELPER_NAME} exited");
        if !restore {
            return; // nothing to put back; the next request spawns a fresh helper
        }
        let me = self.me.clone();
        let _ = thread::Builder::new()
            .name("jezflow-helper-restore".into())
            .spawn(move || {
                thread::sleep(Duration::from_millis(500));
                if let Some(sc) = me.upgrade() {
                    sc.restore_state();
                }
            });
    }

    /// Replays load / setVocabulary / tapStart on a fresh helper.
    fn restore_state(&self) {
        let (version, vocabulary, tap) = {
            let r = self.restore.lock().unwrap();
            (
                r.version.clone(),
                r.vocabulary.clone(),
                r.tap_bundle.clone(),
            )
        };
        let long = Duration::from_secs(600);
        if let Some(v) = version {
            match self.request_blocking("load", json!({ "version": v }), &[], long) {
                Ok(_) => {
                    self.model_ready.store(true, Ordering::SeqCst);
                    tracing::info!(version = %v, "{HELPER_NAME} restarted, model reloaded");
                }
                Err(e) => tracing::error!("{HELPER_NAME} restart: load failed: {e:#}"),
            }
        }
        if !vocabulary.is_empty() {
            if let Err(e) =
                self.request_blocking("setVocabulary", json!({ "terms": vocabulary }), &[], long)
            {
                tracing::error!("{HELPER_NAME} restart: setVocabulary failed: {e:#}");
            }
        }
        if let Some(bundle) = tap {
            if let Err(e) = self.request_blocking(
                "tapStart",
                json!({ "bundle": bundle }),
                &[],
                TAP_START_TIMEOUT,
            ) {
                tracing::error!("{HELPER_NAME} restart: tapStart failed: {e:#}");
            }
        }
    }

    /// Stops the helper and fails everything pending. Further requests error.
    pub fn shutdown(&self) {
        self.closed.store(true, Ordering::SeqCst);
        if let Some(c) = self.conn.lock().unwrap().take() {
            drop(c.frames); // EOF on stdin: the helper tears the tap down and exits
            let child = c.child;
            thread::spawn(move || {
                thread::sleep(Duration::from_secs(2));
                let mut ch = child.lock().unwrap();
                if matches!(ch.try_wait(), Ok(None)) {
                    let _ = ch.kill();
                }
                let _ = ch.wait();
            });
        }
    }

    /// Process id of the running helper (tests use it to simulate a crash).
    pub fn pid(&self) -> Option<u32> {
        self.conn
            .lock()
            .unwrap()
            .as_ref()
            .map(|c| c.child.lock().unwrap().id())
    }

    // ------------------------------------------------------------------ typed calls

    pub async fn ping(&self) -> Result<()> {
        self.request("ping", Value::Null, &[]).await.map(|_| ())
    }

    pub async fn load(&self, version: &str) -> Result<()> {
        self.restore.lock().unwrap().version = Some(version.to_owned());
        self.request("load", json!({ "version": version }), &[])
            .await?;
        self.model_ready.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub async fn transcribe(&self, samples: &[f32], rescore: bool) -> Result<String> {
        let r = self
            .request("transcribe", json!({ "rescore": rescore }), samples)
            .await?;
        Ok(text_of(&r))
    }

    /// Sends the list unless it equals the last one sent (the helper also skips unchanged lists).
    pub async fn set_vocabulary(&self, terms: &[String]) -> Result<()> {
        {
            let mut r = self.restore.lock().unwrap();
            if r.vocabulary == terms {
                return Ok(());
            }
            r.vocabulary = terms.to_vec();
        }
        self.request("setVocabulary", json!({ "terms": terms }), &[])
            .await
            .map(|_| ())
    }

    pub async fn sense_voice(&self, samples: &[f32]) -> Result<String> {
        Ok(text_of(
            &self.request("senseVoice", Value::Null, samples).await?,
        ))
    }

    pub async fn names(&self) -> Result<Vec<String>> {
        let r = self.request("names", Value::Null, &[]).await?;
        Ok(r.get("names")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Number of Core Audio processes whose bundle id matches (debug, `--tapprocs`).
    pub async fn tap_processes(&self, matching: &str) -> Result<usize> {
        let r = self
            .request("tapProcesses", json!({ "match": matching }), &[])
            .await?;
        Ok(r.get("count").and_then(Value::as_u64).unwrap_or(0) as usize)
    }

    pub fn tap_start(&self, bundle: &str, sink: ChunkSink) -> Result<()> {
        *self.tap_sink.lock().unwrap() = Some(sink);
        match self.request_blocking(
            "tapStart",
            json!({ "bundle": bundle }),
            &[],
            TAP_START_TIMEOUT,
        ) {
            Ok(_) => {
                self.restore.lock().unwrap().tap_bundle = Some(bundle.to_owned());
                Ok(())
            }
            Err(e) => {
                *self.tap_sink.lock().unwrap() = None;
                Err(e)
            }
        }
    }

    pub fn tap_stop(&self) {
        self.restore.lock().unwrap().tap_bundle = None;
        // Only talk to a running helper: stopping must not spawn one.
        if self.conn.lock().unwrap().is_some() {
            if let Err(e) = self.request_blocking("tapStop", Value::Null, &[], TAP_STOP_TIMEOUT) {
                tracing::warn!("tapStop: {e:#}");
            }
        }
        // The helper answers after its last tapAudio push, so the sink has seen everything.
        *self.tap_sink.lock().unwrap() = None;
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
        if let Some(c) = self.conn.get_mut().ok().and_then(Option::take) {
            drop(c.frames);
            if let Ok(mut ch) = c.child.lock() {
                let _ = ch.kill();
                let _ = ch.wait();
            }
        }
    }
}

fn text_of(v: &Value) -> String {
    v.get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned()
}

fn writer(stdin: ChildStdin, rx: mpsc::Receiver<Vec<u8>>) {
    let mut w = BufWriter::new(stdin);
    while let Ok(frame) = rx.recv() {
        if w.write_all(&frame).is_err() {
            return;
        }
        // Batch whatever else is already queued, then flush once.
        while let Ok(more) = rx.try_recv() {
            if w.write_all(&more).is_err() {
                return;
            }
        }
        if w.flush().is_err() {
            return;
        }
    }
}

fn reader(me: Weak<Sidecar>, stdout: ChildStdout, generation: u64, child: Arc<Mutex<Child>>) {
    let mut r = BufReader::with_capacity(1 << 16, stdout);
    loop {
        match read_frame(&mut r) {
            Ok(Some(frame)) => match me.upgrade() {
                Some(sc) => sc.dispatch(frame),
                None => return,
            },
            Ok(None) => break,
            Err(e) => {
                tracing::error!("{HELPER_NAME}: bad frame: {e}");
                let _ = child.lock().unwrap().kill();
                break;
            }
        }
    }
    let status = child.lock().unwrap().wait();
    tracing::debug!(?status, generation, "{HELPER_NAME} reader finished");
    if let Some(sc) = me.upgrade() {
        sc.died(generation);
    }
}

// ---------------------------------------------------------------------------------------------- trait impls

/// Parakeet (+ CTC boosting, SenseVoice) in the helper.
#[derive(Clone)]
pub struct SidecarAsr(pub Arc<Sidecar>);

#[async_trait]
impl AsrEngine for SidecarAsr {
    async fn load(&self, version: &str) -> Result<()> {
        self.0.load(version).await
    }

    fn is_ready(&self) -> bool {
        self.0.model_ready.load(Ordering::SeqCst)
    }

    async fn transcribe(&self, samples: &[f32], rescore: bool) -> Result<String> {
        self.0.transcribe(samples, rescore).await
    }

    async fn set_vocabulary(&self, terms: &[String]) -> Result<()> {
        self.0.set_vocabulary(terms).await
    }

    async fn transcribe_multilingual(&self, samples: &[f32]) -> Result<String> {
        self.0.sense_voice(samples).await
    }
}

/// Core Audio process tap in the helper. `start` fails with the helper's message (e.g. no permission).
#[derive(Clone)]
pub struct SidecarSystemAudio(pub Arc<Sidecar>);

impl SystemAudio for SidecarSystemAudio {
    fn start(&self, app_bundle: &str, sink: ChunkSink) -> Result<()> {
        self.0.tap_start(app_bundle, sink)
    }

    fn stop(&self) {
        self.0.tap_stop()
    }
}

/// Contacts + Calendar names from the helper. Empty on any error.
#[derive(Clone)]
pub struct SidecarNames(pub Arc<Sidecar>);

#[async_trait]
impl NameSource for SidecarNames {
    async fn names(&self) -> Vec<String> {
        match self.0.names().await {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!("names: {e:#}");
                Vec::new()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_round_trip_with_audio() {
        let audio = vec![0.5f32, -1.0, 0.25];
        let bytes = encode_frame(
            &json!({"id": 7, "method": "transcribe", "params": {"rescore": true}}),
            &audio,
        );
        let len = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
        assert_eq!(bytes.len(), 4 + len + 12);
        let f = read_frame(&mut &bytes[..]).unwrap().unwrap();
        assert_eq!(f.header["audio"], 3);
        assert_eq!(f.header["method"], "transcribe");
        assert_eq!(f.audio, audio);
    }

    #[test]
    fn frame_without_audio_has_no_audio_key() {
        let bytes = encode_frame(&json!({"id": 1, "method": "ping"}), &[]);
        let f = read_frame(&mut &bytes[..]).unwrap().unwrap();
        assert!(f.header.get("audio").is_none());
        assert!(f.audio.is_empty());
    }

    #[test]
    fn read_frame_eof_and_sequence() {
        let mut bytes = encode_frame(&json!({"event": "tapRestarted"}), &[]);
        bytes.extend(encode_frame(&json!({"event": "tapAudio"}), &[1.0; 1600]));
        let mut r = &bytes[..];
        assert_eq!(
            read_frame(&mut r).unwrap().unwrap().header["event"],
            "tapRestarted"
        );
        assert_eq!(read_frame(&mut r).unwrap().unwrap().audio.len(), 1600);
        assert!(read_frame(&mut r).unwrap().is_none());
    }

    #[test]
    fn missing_helper_errors() {
        let sc = Sidecar::with_path(None);
        let err = sc
            .request_blocking("ping", Value::Null, &[], Duration::from_secs(1))
            .unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    fn built_helper() -> Option<PathBuf> {
        let p = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../sidecar/jezflow-helper/.build/release/jezflow-helper");
        p.is_file().then_some(p)
    }

    /// Needs `swift build -c release` in sidecar/jezflow-helper; skipped otherwise.
    #[tokio::test]
    async fn helper_ping_unknown_method_and_restart() {
        let Some(path) = built_helper() else {
            eprintln!("helper not built; skipping");
            return;
        };
        let sc = Sidecar::with_path(Some(path));
        sc.ping().await.unwrap();
        let err = sc.request("nope", Value::Null, &[]).await.unwrap_err();
        assert!(err.to_string().contains("unknown method"), "{err}");
        // process listing needs no permission; a bundle id that can't exist matches nothing
        assert_eq!(
            sc.tap_processes("com.example.nothing.here").await.unwrap(),
            0
        );
        // transcribe before load: the helper's own error comes back
        let err = sc.transcribe(&[0.0; 1600], false).await.unwrap_err();
        assert!(err.to_string().contains("still loading"), "{err}");
        // crash: the next request spawns a fresh helper
        let pid = sc.pid().unwrap();
        std::process::Command::new("kill")
            .arg("-9")
            .arg(pid.to_string())
            .status()
            .unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        sc.ping().await.unwrap();
        assert_ne!(sc.pid().unwrap(), pid);
        sc.shutdown();
    }
}
