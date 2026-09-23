//! OWNER: team A (rules). Whisper hallucination filter (TextRules.filterWhisper), shared by Groq and whisper-server.

use once_cell::sync::Lazy;
use serde::Deserialize;

use crate::text_rules::util::{trim_hspace, trim_punct};

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

/// Phrases Whisper produces from silence or noise (its training data ends of videos).
const WHISPER_GHOSTS: &[&str] = &[
    "thank you", "thanks for watching", "thank you for watching", "bye", "you", "the end",
    "subtitles by the amara.org community", "please subscribe", "like and subscribe", "thank you very much", "i'm sorry",
    "so", "oh", "um",
];

static SPACES: Lazy<regex::Regex> = Lazy::new(|| regex::Regex::new(r"\s{2,}").expect("regex"));

fn ghost(t: &str) -> bool {
    let lower = t.to_lowercase();
    WHISPER_GHOSTS.contains(&trim_hspace(trim_punct(&lower)))
}

/// Drops segments Whisper itself flags as unlikely speech (its standard thresholds), plus known ghost phrases and
/// implausible speaking rates. An empty `segments` behaves like Swift's `nil`.
pub fn filter_whisper(text: &str, segments: &[WhisperSegment], seconds: f64) -> String {
    let mut out: Vec<String> = Vec::new();
    if !segments.is_empty() {
        for seg in segments {
            let t = trim_hspace(&seg.text);
            if t.is_empty() {
                continue;
            }
            if seg.no_speech_prob > 0.6 && seg.avg_logprob < -0.7 {
                continue; // Whisper's own "probably not speech" rule
            }
            if seg.avg_logprob < -1.2 {
                continue; // very low confidence
            }
            if seg.compression_ratio > 2.4 {
                continue; // repetitive babble
            }
            if ghost(t) {
                continue;
            }
            out.push(t.to_string());
        }
    } else if !ghost(text) {
        out = vec![text.trim().to_string()];
    }
    let joined = SPACES.replace_all(&out.join(" "), " ").into_owned();
    let words = joined.split(' ').filter(|w| !w.is_empty()).count() as f64;
    if seconds > 1.0 && words / seconds > 6.0 {
        return String::new(); // nobody speaks 6+ words a second
    }
    joined
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(text: &str, no_speech: f64, logprob: f64) -> WhisperSegment {
        WhisperSegment { text: text.into(), no_speech_prob: no_speech, avg_logprob: logprob, compression_ratio: 1.0 }
    }

    #[test]
    fn babble_and_json_defaults() {
        let s: WhisperSegment = serde_json::from_str(r#"{"text":" hi"}"#).unwrap();
        assert_eq!(s.compression_ratio, 1.0);
        let mut b = seg(" la la la", 0.0, -0.1);
        b.compression_ratio = 3.0;
        assert_eq!(filter_whisper("x", &[b, seg(" real words", 0.0, -0.1)], 3.0), "real words");
    }
}
