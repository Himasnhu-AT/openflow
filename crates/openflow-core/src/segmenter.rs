//! OWNER: team B (data). Energy VAD that splits a 16 kHz stream into utterances (MeetingAudio.swift Segmenter).
//! Thread-safe: `push` is called from audio threads.

use std::sync::Mutex;

/// (samples, start seconds from stream start, seconds of audio already processed after the utterance's end).
/// The last value lets a caller place the utterance on a wall clock: its end was `behind` seconds before now.
pub type UtteranceSink = Box<dyn Fn(Vec<f32>, f64, f64) + Send + Sync>;

/// 20 ms at 16 kHz.
pub const FRAME: usize = 320;
/// 700 ms of quiet ends an utterance.
pub const END_SILENCE: usize = 35;
/// Utterances are cut at 25 s, under Whisper's 30 s window.
pub const MAX_SAMPLES: usize = 25 * 16_000;
/// A cut at the cap goes at the quietest frame of its last 4 s, so a word is not split in half.
const CUT_SEARCH: usize = 200;
/// 300 ms of audio kept before speech starts.
pub const PRE_ROLL: usize = 4800;
const INITIAL_NOISE: f32 = 0.004;
const MIN_NOISE: f32 = 0.002;

/// Swift defaults (`speechFloor` 0.008, `minSpeechFrames` 12); the system-audio tap uses 0.010 / 15.
pub const DEFAULT_SPEECH_FLOOR: f32 = 0.008;
pub const DEFAULT_MIN_SPEECH_FRAMES: usize = 12;

struct State {
    buffer: Vec<f32>,
    /// RMS of each FRAME of `buffer` (the buffer only ever holds whole frames).
    frame_rms: Vec<f32>,
    /// Whether each frame of `buffer` counted as speech.
    frame_speech: Vec<bool>,
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
                frame_rms: Vec::new(),
                frame_speech: Vec::new(),
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
            st.frame_rms.push(rms);
            st.frame_speech.push(speech);
            st.elapsed += FRAME;
            if speech {
                st.speech_frames += 1;
                st.silent_frames = 0;
            } else {
                st.silent_frames += 1;
            }
            if st.speech_frames > 0 && st.silent_frames >= END_SILENCE {
                self.flush(st, st.frame_rms.len());
            } else if st.speech_frames > 0 && st.buffer.len() >= MAX_SAMPLES {
                let cut = quietest_cut(&st.frame_rms);
                self.flush(st, cut);
            } else if st.speech_frames == 0 && st.buffer.len() > PRE_ROLL {
                let extra = st.buffer.len() - PRE_ROLL;
                st.buffer.drain(..extra); // keep 300 ms of pre-roll while nobody talks
                st.frame_rms.drain(..extra / FRAME);
                st.frame_speech.drain(..extra / FRAME);
            }
            i += FRAME;
        }
        st.pending.drain(..i);
    }

    /// While an utterance is being collected: how many seconds of audio ago it began. None between utterances.
    pub fn speech_started_ago(&self) -> Option<f64> {
        let st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        (st.speech_frames > 0).then(|| st.buffer.len() as f64 / 16_000.0)
    }

    /// Flush what is left.
    pub fn finish(&self) {
        let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let st = &mut *guard;
        let frames = st.frame_rms.len();
        self.flush(st, frames);
    }

    /// Emits the first `frames` frames of the buffer (when they hold enough speech) and keeps the rest as the
    /// start of the next utterance.
    fn flush(&self, st: &mut State, frames: usize) {
        let buffered_start = st.elapsed - st.buffer.len();
        let head_speech = st.frame_speech[..frames].iter().filter(|s| **s).count();
        let head: Vec<f32> = st.buffer.drain(..frames * FRAME).collect();
        st.frame_rms.drain(..frames);
        st.frame_speech.drain(..frames);
        if head_speech >= self.min_speech_frames {
            let start = buffered_start as f64 / 16_000.0;
            let behind = st.buffer.len() as f64 / 16_000.0;
            (self.on_utterance)(head, start, behind);
        }
        st.speech_frames = st.frame_speech.iter().filter(|s| **s).count();
        st.silent_frames = st.frame_speech.iter().rev().take_while(|s| !**s).count();
    }
}

/// Frame count to emit when an utterance hits the cap: just after the quietest frame of the last `CUT_SEARCH`
/// frames when that frame is a real dip (under half the stretch's average), otherwise at the cap itself.
fn quietest_cut(rms: &[f32]) -> usize {
    let window = &rms[rms.len().saturating_sub(CUT_SEARCH)..];
    let mean = window.iter().sum::<f32>() / window.len() as f32;
    let (i, min) = window.iter().enumerate().fold((0, f32::MAX), |b, (i, r)| if *r < b.1 { (i, *r) } else { b });
    if min < mean * 0.5 {
        rms.len() - window.len() + i + 1
    } else {
        rms.len()
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
        (Segmenter::new(Box::new(move |s, t, _| o.lock().unwrap().push((s.len(), t)))), out)
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
        feed(&s, &audio[..16_000 * 2]);
        assert!((s.speech_started_ago().unwrap() - (PRE_ROLL + 16_000) as f64 / 16_000.0).abs() < 1e-9);
        feed(&s, &audio[16_000 * 2..]);
        assert_eq!(s.speech_started_ago(), None);
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
    fn long_speech_is_cut_at_the_cap() {
        let (s, out) = seg();
        feed(&s, &tone(16_000 * 60, 0.1));
        s.finish();
        let out = out.lock().unwrap();
        assert_eq!(out.iter().map(|o| o.0).collect::<Vec<_>>(), [MAX_SAMPLES, MAX_SAMPLES, 16_000 * 10]);
        assert_eq!(out.iter().map(|o| o.1).collect::<Vec<_>>(), [0.0, 25.0, 50.0]);
    }

    #[test]
    fn cap_cuts_at_the_pause_between_words() {
        let out: Arc<StdMutex<Vec<(usize, f64, f64)>>> = Arc::default();
        let o = out.clone();
        let s = Segmenter::new(Box::new(move |x, t, b| o.lock().unwrap().push((x.len(), t, b))));
        // 23 s of talk, a 200 ms breath (too short to end the utterance), then 5 s more talk.
        let mut audio = tone(16_000 * 23, 0.1);
        audio.extend(tone(3200, 0.001));
        audio.extend(tone(16_000 * 5, 0.1));
        feed(&s, &audio);
        let got = out.lock().unwrap().clone();
        assert_eq!(got.len(), 1);
        // Cut inside the breath, not mid-word at 25 s; the rest waits for the next utterance.
        let first = got[0].0;
        assert!(first > 16_000 * 23 && first <= 16_000 * 23 + 3200, "{first}");
        assert!((got[0].2 - (MAX_SAMPLES - first) as f64 / 16_000.0).abs() < 1e-9);
        drop(got);
        s.finish();
        let got = out.lock().unwrap();
        assert_eq!(got[1].0 + first, audio.len());
        assert!((got[1].1 - first as f64 / 16_000.0).abs() < 1e-9);
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
