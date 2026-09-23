//! OWNER: team A (rules). Whisper hallucination filter (TextRules.filterWhisper), shared by Groq and whisper-server.

use serde::Deserialize;

/// One `segments[]` entry of a verbose_json response. Missing fields use the Swift defaults.
#[derive(Debug, Clone, Deserialize)]
pub struct WhisperSegment {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub no_speech_prob: f64,
    #[serde(default)]
    pub avg_logprob: f64,
    #[serde(default = "one")]
    pub compression_ratio: f64,
}

fn one() -> f64 {
    1.0
}

pub fn filter_whisper(_text: &str, _segments: &[WhisperSegment], _seconds: f64) -> String {
    todo!("team A")
}
