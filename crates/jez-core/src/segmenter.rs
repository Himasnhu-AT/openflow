//! OWNER: team B (data). Energy VAD that splits a 16 kHz stream into utterances (MeetingAudio.swift Segmenter).
//! Thread-safe: `push` is called from audio threads.

/// (samples, start seconds from stream start)
pub type UtteranceSink = Box<dyn Fn(Vec<f32>, f64) + Send + Sync>;

pub struct Segmenter {
    pub speech_floor: f32,
    pub min_speech_frames: usize,
}

impl Segmenter {
    pub fn new(_on_utterance: UtteranceSink) -> Self {
        todo!("team B")
    }
    pub fn push(&self, _chunk: &[f32]) {
        todo!("team B")
    }
    /// Flush what is left.
    pub fn finish(&self) {
        todo!("team B")
    }
}
