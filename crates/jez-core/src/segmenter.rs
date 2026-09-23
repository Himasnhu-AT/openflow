//! OWNER: team B (data). Energy VAD that splits a 16 kHz stream into utterances (MeetingAudio.swift Segmenter).
//! Thread-safe: `push` is called from audio threads.

use std::sync::Mutex;

/// (samples, start seconds from stream start)
pub type UtteranceSink = Box<dyn Fn(Vec<f32>, f64) + Send + Sync>;

/// 20 ms at 16 kHz.
pub const FRAME: usize = 320;
/// 700 ms of quiet ends an utterance.
pub const END_SILENCE: usize = 35;
/// Utterances are cut at 20 s.
pub const MAX_SAMPLES: usize = 20 * 16_000;
/// 300 ms of audio kept before speech starts.
pub const PRE_ROLL: usize = 4800;
const INITIAL_NOISE: f32 = 0.004;
const MIN_NOISE: f32 = 0.002;

/// Swift defaults (`speechFloor` 0.008, `minSpeechFrames` 12); the system-audio tap uses 0.010 / 15.
pub const DEFAULT_SPEECH_FLOOR: f32 = 0.008;
pub const DEFAULT_MIN_SPEECH_FRAMES: usize = 12;

struct State {
    buffer: Vec<f32>,
    silent_frames: usize,
    speech_frames: usize,
    noise: f32,
    /// Samples processed, for timestamps.
    elapsed: usize,
    /// Audio arrives in arbitrary chunk sizes (the tap sends ~150 samples).
    pending: Vec<f32>,
}

pub struct Segmenter {
    /// Minimum RMS for a frame to count as speech (call audio through the tap uses a higher bar than a mic).
    /// Set before the segmenter is shared.
    pub speech_floor: f32,
    /// Fewer speech frames than this is too short to be a word; such utterances are dropped.
    pub min_speech_frames: usize,
    state: Mutex<State>,
    on_utterance: UtteranceSink,
}

impl Segmenter {
    pub fn new(on_utterance: UtteranceSink) -> Self {
        Segmenter {
            speech_floor: DEFAULT_SPEECH_FLOOR,
            min_speech_frames: DEFAULT_MIN_SPEECH_FRAMES,
            state: Mutex::new(State {
                buffer: Vec::new(),
                silent_frames: 0,
                speech_frames: 0,
                noise: INITIAL_NOISE,
                elapsed: 0,
                pending: Vec::new(),
            }),
            on_utterance,
        }
    }

    /// The sink runs on the calling thread with the lock held (as in Swift), so utterances arrive in order;
    /// it must not call back into this segmenter.
    pub fn push(&self, chunk: &[f32]) {
        let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let st = &mut *guard;
        st.pending.extend_from_slice(chunk);
        let mut i = 0;
        while i + FRAME <= st.pending.len() {
            let f = &st.pending[i..i + FRAME];
            let rms = (f.iter().fold(0f32, |acc, x| acc + x * x) / FRAME as f32).sqrt();
            if rms < st.noise * 1.5 {
                st.noise = MIN_NOISE.max(st.noise * 0.995 + rms * 0.005); // track the floor slowly
            }
            let speech = rms > self.speech_floor.max(st.noise * 4.0);
            st.buffer.extend_from_slice(f);
            st.elapsed += FRAME;
            if speech {
                st.speech_frames += 1;
                st.silent_frames = 0;
            } else {
                st.silent_frames += 1;
            }
            if st.speech_frames > 0 && (st.silent_frames >= END_SILENCE || st.buffer.len() >= MAX_SAMPLES) {
                let end = st.elapsed;
                self.flush(&mut st.buffer, &mut st.speech_frames, &mut st.silent_frames, end);
            } else if st.speech_frames == 0 && st.buffer.len() > PRE_ROLL {
                let extra = st.buffer.len() - PRE_ROLL;
                st.buffer.drain(..extra); // keep 300 ms of pre-roll while nobody talks
            }
            i += FRAME;
        }
        st.pending.drain(..i);
    }

    /// Flush what is left.
    pub fn finish(&self) {
        let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let st = &mut *guard;
        let end = st.elapsed;
        self.flush(&mut st.buffer, &mut st.speech_frames, &mut st.silent_frames, end);
    }

    fn flush(&self, buffer: &mut Vec<f32>, speech_frames: &mut usize, silent_frames: &mut usize, abs_end: usize) {
        if *speech_frames >= self.min_speech_frames {
            let start = (abs_end as f64 - buffer.len() as f64).max(0.0) / 16_000.0;
            (self.on_utterance)(buffer.clone(), start);
        }
        buffer.clear();
        *speech_frames = 0;
        *silent_frames = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex as StdMutex};

    type Out = Arc<StdMutex<Vec<(usize, f64)>>>;

    fn seg() -> (Segmenter, Out) {
        let out: Out = Arc::default();
        let o = out.clone();
        (Segmenter::new(Box::new(move |s, t| o.lock().unwrap().push((s.len(), t)))), out)
    }

    fn silence(n: usize) -> Vec<f32> {
        vec![0.0; n]
    }

    /// A 440 Hz tone at `amp` (RMS = amp / sqrt 2).
    fn tone(n: usize, amp: f32) -> Vec<f32> {
        (0..n).map(|i| amp * (i as f32 * 2.0 * std::f32::consts::PI * 440.0 / 16_000.0).sin()).collect()
    }

    /// Feeds in awkward chunk sizes, like the tap's ~150-sample buffers.
    fn feed(s: &Segmenter, audio: &[f32]) {
        for c in audio.chunks(150) {
            s.push(c);
        }
    }

    #[test]
    fn silence_emits_nothing() {
        let (s, out) = seg();
        feed(&s, &silence(16_000 * 30));
        s.finish();
        assert!(out.lock().unwrap().is_empty());
    }

    #[test]
    fn burst_with_pre_roll_and_trailing_silence() {
        let (s, out) = seg();
        let mut audio = silence(16_000); // 1 s = 50 frames
        audio.extend(tone(16_000, 0.1)); // 1 s speech = 50 frames
        audio.extend(silence(16_000));
        feed(&s, &audio);
        s.finish();
        let out = out.lock().unwrap();
        assert_eq!(out.len(), 1);
        // pre-roll (4800) + speech (16000) + 35 silent frames (11200)
        assert_eq!(out[0].0, PRE_ROLL + 16_000 + END_SILENCE * FRAME);
        assert!((out[0].1 - (16_000 - PRE_ROLL) as f64 / 16_000.0).abs() < 1e-9, "{}", out[0].1);
    }

    #[test]
    fn short_blip_is_dropped() {
        let (s, out) = seg();
        let mut audio = silence(8000);
        audio.extend(tone(FRAME * 11, 0.1)); // 11 frames < 12
        audio.extend(silence(16_000));
        audio.extend(tone(FRAME * 12, 0.1)); // exactly 12 frames: kept
        audio.extend(silence(16_000));
        feed(&s, &audio);
        let out = out.lock().unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, PRE_ROLL + 12 * FRAME + END_SILENCE * FRAME);
    }

    #[test]
    fn long_speech_is_cut_at_20_seconds() {
        let (s, out) = seg();
        feed(&s, &tone(16_000 * 45, 0.1));
        s.finish();
        let out = out.lock().unwrap();
        assert_eq!(out.iter().map(|o| o.0).collect::<Vec<_>>(), [MAX_SAMPLES, MAX_SAMPLES, 16_000 * 5]);
        assert_eq!(out.iter().map(|o| o.1).collect::<Vec<_>>(), [0.0, 20.0, 40.0]);
    }

    #[test]
    fn finish_flushes_pending_speech_but_not_partial_frames() {
        let (s, out) = seg();
        let mut audio = tone(FRAME * 20, 0.1);
        audio.extend(tone(100, 0.1)); // partial frame stays pending
        feed(&s, &audio);
        assert!(out.lock().unwrap().is_empty());
        s.finish();
        assert_eq!(*out.lock().unwrap(), [(FRAME * 20, 0.0)]);
        s.finish(); // nothing left
        assert_eq!(out.lock().unwrap().len(), 1);
    }

    #[test]
    fn speech_floor_is_settable() {
        let (mut s, out) = seg();
        s.speech_floor = 0.1; // tone RMS ~0.07 is below it
        s.min_speech_frames = 15;
        feed(&s, &tone(16_000, 0.1));
        s.finish();
        assert!(out.lock().unwrap().is_empty());
    }

    #[test]
    fn noise_floor_adapts_to_steady_hum() {
        let (s, out) = seg();
        // A quiet hum lowers the noise estimate (never below 0.002) without counting as speech; speech on top
        // of it is detected and timed from the end of the 10 s hum minus the pre-roll.
        let hum = tone(16_000 * 10, 0.0005);
        feed(&s, &hum);
        let mut audio = tone(16_000, 0.2);
        audio.extend(tone(16_000, 0.0005));
        feed(&s, &audio);
        let out = out.lock().unwrap();
        assert_eq!(out.len(), 1);
        assert!((out[0].1 - (10.0 - 0.3)).abs() < 1e-9);
    }

    #[test]
    fn thread_safe_pushes() {
        let (s, out) = seg();
        let s = Arc::new(s);
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let s = s.clone();
                std::thread::spawn(move || {
                    for _ in 0..100 {
                        s.push(&silence(FRAME));
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        s.finish();
        assert!(out.lock().unwrap().is_empty());
    }
}
