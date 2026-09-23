//! Microphone (AudioRecorder.swift) on cpal/CoreAudio, delivering 16 kHz mono f32.
//!
//! One dedicated thread owns the cpal stream (build, play, pause, drop), driven by commands, so `start` never
//! blocks the caller on a cold Bluetooth mic. After `stop` the stream keeps running for 20 s (audio discarded)
//! so the next recording captures from the first word: AirPods and other Bluetooth mics deliver ~1 s of silence
//! when they wake.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::anyhow;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{DeviceId, HostId, SampleFormat};
use jez_core::traits::{ChunkSink, LevelSink, Mic};
use rubato::{FftFixedInOut, Resampler};

use super::devices::{resolve_mac, MacInput};

const TARGET_RATE: usize = 16_000;
const KEEP_WARM: Duration = Duration::from_secs(20);

enum Cmd {
    Start(MacInput),
    Stop,
    Shutdown,
}

#[derive(Default)]
struct Capture {
    samples: Vec<f32>,
    capturing: bool,
    second_samples: usize,
    second_peak: f32,
}

#[derive(Default)]
struct Shared {
    capture: Mutex<Capture>,
    on_level: RwLock<Option<LevelSink>>,
    on_chunk: RwLock<Option<ChunkSink>>,
    running: AtomicBool,
    /// The stream's device reported an error (disconnected): rebuild on the next start.
    stale: AtomicBool,
    device_uid: Mutex<Option<String>>,
}

impl Shared {
    /// One 16 kHz mono chunk from the audio thread.
    fn deliver(&self, chunk: &[f32]) {
        if chunk.is_empty() {
            return;
        }
        let active = {
            let mut c = self.capture.lock().unwrap();
            let active = c.capturing;
            if active {
                c.samples.extend_from_slice(chunk);
            }
            c.second_samples += chunk.len();
            let peak = chunk.iter().fold(0f32, |m, x| m.max(x.abs()));
            c.second_peak = c.second_peak.max(peak);
            if c.second_samples >= TARGET_RATE {
                tracing::info!(target: "audio", "mic: {} samples, peak {}, capturing {active}", c.second_samples, c.second_peak);
                c.second_samples = 0;
                c.second_peak = 0.0;
            }
            active
        };
        if let Some(sink) = self.on_chunk.read().unwrap().as_ref() {
            sink(chunk);
        }
        if active {
            if let Some(sink) = self.on_level.read().unwrap().as_ref() {
                let rms = (chunk.iter().map(|x| x * x).sum::<f32>() / chunk.len() as f32).sqrt();
                sink(rms);
            }
        }
    }
}

/// Downmix to mono and resample to 16 kHz.
struct Converter {
    channels: usize,
    resampler: Option<FftFixedInOut<f32>>,
    pending: Vec<f32>,
    mono: Vec<f32>,
}

impl Converter {
    fn new(rate: usize, channels: usize) -> anyhow::Result<Converter> {
        let resampler = if rate == TARGET_RATE {
            None
        } else {
            Some(FftFixedInOut::<f32>::new(rate, TARGET_RATE, (rate / 100).max(1), 1)?)
        };
        Ok(Converter { channels: channels.max(1), resampler, pending: Vec::new(), mono: Vec::new() })
    }

    fn push<T: Copy>(&mut self, data: &[T], to_f32: impl Fn(T) -> f32) -> Vec<f32> {
        self.mono.clear();
        let ch = self.channels;
        self.mono.extend(data.chunks(ch).map(|f| f.iter().map(|s| to_f32(*s)).sum::<f32>() / f.len() as f32));
        let Some(rs) = self.resampler.as_mut() else { return self.mono.clone() };
        self.pending.extend_from_slice(&self.mono);
        let mut out = Vec::new();
        loop {
            let need = rs.input_frames_next();
            if self.pending.len() < need {
                break;
            }
            match rs.process(&[&self.pending[..need]], None) {
                Ok(mut v) => out.append(&mut v[0]),
                Err(e) => tracing::error!(target: "audio", "resample failed: {e}"),
            }
            self.pending.drain(..need);
        }
        out
    }
}

fn build_stream(input: &MacInput, shared: &Arc<Shared>) -> anyhow::Result<cpal::Stream> {
    let host = cpal::host_from_id(HostId::CoreAudio)?;
    let device = host
        .device_by_id(&DeviceId::new(HostId::CoreAudio, &input.device.uid))
        .ok_or_else(|| anyhow!("Microphone not available (is it connected and awake?)"))?;
    let supported = device.default_input_config()?;
    let format = supported.sample_format();
    let config = supported.config();
    let mut conv = Converter::new(config.sample_rate as usize, config.channels as usize)?;
    tracing::info!(target: "audio", "Microphone: {} ({} Hz, {} ch, {format:?})", input.device.name, config.sample_rate, config.channels);
    let data_shared = Arc::clone(shared);
    let err_shared = Arc::clone(shared);
    let stream = device.build_input_stream_raw(
        config,
        format,
        move |data: &cpal::Data, _| {
            let out = match data.sample_format() {
                SampleFormat::F32 => data.as_slice::<f32>().map(|d| conv.push(d, |s| s)),
                SampleFormat::I16 => data.as_slice::<i16>().map(|d| conv.push(d, |s| s as f32 / 32768.0)),
                SampleFormat::I32 => data.as_slice::<i32>().map(|d| conv.push(d, |s| s as f32 / 2_147_483_648.0)),
                SampleFormat::F64 => data.as_slice::<f64>().map(|d| conv.push(d, |s| s as f32)),
                _ => None,
            };
            if let Some(out) = out {
                data_shared.deliver(&out);
            }
        },
        move |err| {
            tracing::error!(target: "audio", "Capture event: {err}");
            if matches!(err.kind(), cpal::ErrorKind::DeviceNotAvailable) {
                err_shared.stale.store(true, Ordering::SeqCst); // rebuild on the next start
            }
        },
        None,
    )?;
    Ok(stream)
}

fn run(rx: mpsc::Receiver<Cmd>, shared: Arc<Shared>) {
    let mut stream: Option<(cpal::Stream, MacInput)> = None;
    let mut warm_until: Option<Instant> = None;
    loop {
        let cmd = match warm_until {
            Some(deadline) => match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(c) => Some(c),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            },
            None => match rx.recv() {
                Ok(c) => Some(c),
                Err(_) => break,
            },
        };
        match cmd {
            None => {
                // Kept warm long enough: stop the device (the stream is kept for the next start).
                warm_until = None;
                if let Some((s, _)) = &stream {
                    let _ = s.pause();
                }
                shared.running.store(false, Ordering::SeqCst);
            }
            Some(Cmd::Start(input)) => {
                warm_until = None;
                let rebuild = match &stream {
                    None => true,
                    Some((_, cur)) => cur.id != input.id || cur.device.uid != input.device.uid,
                } || shared.stale.swap(false, Ordering::SeqCst);
                if rebuild {
                    stream = None;
                    match build_stream(&input, &shared) {
                        Ok(s) => {
                            *shared.device_uid.lock().unwrap() = Some(input.device.uid.clone());
                            stream = Some((s, input));
                        }
                        Err(e) => {
                            tracing::error!(target: "audio", "Can't capture from {}: {e:#}", input.device.name);
                            *shared.device_uid.lock().unwrap() = None;
                            shared.running.store(false, Ordering::SeqCst);
                            continue;
                        }
                    }
                }
                if let Some((s, _)) = &stream {
                    match s.play() {
                        Ok(()) => shared.running.store(true, Ordering::SeqCst),
                        Err(e) => {
                            tracing::error!(target: "audio", "Microphone failed to start: {e}");
                            shared.stale.store(true, Ordering::SeqCst);
                            shared.running.store(false, Ordering::SeqCst);
                        }
                    }
                }
            }
            Some(Cmd::Stop) => warm_until = Some(Instant::now() + KEEP_WARM),
            Some(Cmd::Shutdown) => {
                warm_until = None;
                if let Some((s, _)) = &stream {
                    let _ = s.pause();
                }
                shared.running.store(false, Ordering::SeqCst);
            }
        }
    }
}

pub struct MacMic {
    shared: Arc<Shared>,
    tx: Mutex<mpsc::Sender<Cmd>>,
}

impl Default for MacMic {
    fn default() -> Self {
        Self::new()
    }
}

impl MacMic {
    pub fn new() -> MacMic {
        let shared = Arc::new(Shared::default());
        let (tx, rx) = mpsc::channel();
        let s2 = Arc::clone(&shared);
        thread::Builder::new().name("jezflow-mic".into()).spawn(move || run(rx, s2)).expect("spawn mic thread");
        MacMic { shared, tx: Mutex::new(tx) }
    }

    fn send(&self, cmd: Cmd) {
        let _ = self.tx.lock().unwrap().send(cmd);
    }
}

impl Mic for MacMic {
    /// "" = system default, "builtin" = the Mac's own mic, or a device UID (falls back to the default when that
    /// device is gone).
    fn start(&self, preferred_device: &str) -> anyhow::Result<()> {
        let input = resolve_mac(preferred_device)
            .or_else(|| resolve_mac(""))
            .ok_or_else(|| anyhow!("Microphone not available (is it connected and awake?)"))?;
        {
            let mut c = self.shared.capture.lock().unwrap();
            c.samples.clear();
            c.capturing = true;
        }
        self.send(Cmd::Start(input));
        Ok(())
    }

    fn snapshot(&self) -> Vec<f32> {
        self.shared.capture.lock().unwrap().samples.clone()
    }

    fn stop(&self) -> Vec<f32> {
        let out = {
            let mut c = self.shared.capture.lock().unwrap();
            c.capturing = false;
            std::mem::take(&mut c.samples)
        };
        self.send(Cmd::Stop);
        out
    }

    fn shutdown(&self) {
        self.send(Cmd::Shutdown);
    }

    fn is_running(&self) -> bool {
        self.shared.running.load(Ordering::SeqCst)
    }

    fn set_on_level(&self, sink: Option<LevelSink>) {
        *self.shared.on_level.write().unwrap() = sink;
    }

    fn set_on_chunk(&self, sink: Option<ChunkSink>) {
        *self.shared.on_chunk.write().unwrap() = sink;
    }

    fn current_device_uid(&self) -> Option<String> {
        self.shared.device_uid.lock().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converter_downmixes_and_resamples() {
        let mut c = Converter::new(48_000, 2).unwrap();
        // 1 s of stereo 440 Hz.
        let data: Vec<f32> = (0..48_000)
            .flat_map(|i| {
                let v = (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.5;
                [v, v]
            })
            .collect();
        let mut out = Vec::new();
        for chunk in data.chunks(960) {
            out.extend(c.push(chunk, |s| s));
        }
        assert!((15_000..=16_000).contains(&out.len()), "{}", out.len());
        let peak = out[2000..].iter().fold(0f32, |m, x| m.max(x.abs()));
        assert!((0.45..0.55).contains(&peak), "{peak}");
    }

    #[test]
    fn converter_passthrough_at_16k() {
        let mut c = Converter::new(16_000, 1).unwrap();
        assert_eq!(c.push(&[1i16 << 14, -(1 << 14)], |s| s as f32 / 32768.0), vec![0.5, -0.5]);
    }

    #[test]
    fn deliver_captures_and_reports() {
        let shared = Shared::default();
        let chunks = Arc::new(Mutex::new(0usize));
        let levels = Arc::new(Mutex::new(Vec::new()));
        let (c2, l2) = (Arc::clone(&chunks), Arc::clone(&levels));
        *shared.on_chunk.write().unwrap() = Some(Arc::new(move |_| *c2.lock().unwrap() += 1));
        *shared.on_level.write().unwrap() = Some(Arc::new(move |v| l2.lock().unwrap().push(v)));
        shared.deliver(&[0.5, -0.5]);
        assert_eq!(*chunks.lock().unwrap(), 1);
        assert!(levels.lock().unwrap().is_empty());
        assert!(shared.capture.lock().unwrap().samples.is_empty());
        shared.capture.lock().unwrap().capturing = true;
        shared.deliver(&[0.5, -0.5]);
        assert_eq!(*chunks.lock().unwrap(), 2);
        assert_eq!(*levels.lock().unwrap(), vec![0.5]);
        assert_eq!(shared.capture.lock().unwrap().samples, vec![0.5, -0.5]);
    }
}
