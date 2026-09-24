//! OWNER: team A (rules). Whisper hallucination filter (TextRules.filterWhisper), shared by Groq and the meeting speech server.

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
/// Sound annotations Whisper writes instead of speech: [BLANK_AUDIO], (music), *sighs*, ♪ lyrics ♪.
static ANNOTATIONS: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r"\[[^\]]*\]|\([^)]*\)|\*[^*]*\*|♪[^♪]*♪?").expect("regex"));

/// Removes sound annotations; empty when no letter or digit is left.
pub fn strip_annotations(text: &str) -> String {
    let t = ANNOTATIONS.replace_all(text, " ");
    let t = SPACES.replace_all(t.trim(), " ").into_owned();
    if t.chars().any(|c| c.is_alphanumeric()) {
        t
    } else {
        String::new()
    }
}

fn norm_word(w: &str) -> String {
    w.chars().filter(|c| c.is_alphanumeric() || *c == '\'').flat_map(char::to_lowercase).collect()
}

/// Decoder loops: an immediately repeated run of 3–10 words is dropped, and a word or word pair repeated four or
/// more times in a row is cut back to one. "No, no" and "very very" survive.
pub fn collapse_repeats(text: &str) -> String {
    let mut words: Vec<&str> = text.split_whitespace().collect();
    let mut changed = true;
    while changed {
        changed = false;
        let keys: Vec<String> = words.iter().map(|w| norm_word(w)).collect();
        'scan: for n in 1..=10 {
            let min_copies = if n >= 3 { 2 } else { 4 };
            for i in 0..words.len() {
                let mut copies = 1;
                while i + (copies + 1) * n <= keys.len()
                    && keys[i..i + n].iter().all(|k| !k.is_empty())
                    && keys[i..i + n] == keys[i + copies * n..i + (copies + 1) * n]
                {
                    copies += 1;
                }
                if copies >= min_copies {
                    // Keep the last copy: it carries the sentence's closing punctuation.
                    words.drain(i..i + (copies - 1) * n);
                    changed = true;
                    break 'scan;
                }
            }
        }
    }
    words.join(" ")
}

/// Nobody dictates or says a bare web address; Whisper writes one for noise (the ends of its training videos).
static BARE_URL: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r"(?i)^(https?://|www\.)?[a-z0-9-]+(\.[a-z0-9-]+)*\.(com|org|net|info|io|tv|co|uk|de|ru)(/\S*)?$").expect("regex"));

fn ghost(t: &str) -> bool {
    let lower = t.to_lowercase();
    let core = trim_hspace(trim_punct(&lower));
    WHISPER_GHOSTS.contains(&core) || BARE_URL.is_match(core) || (lower.contains("www.") && core.split_whitespace().count() <= 2)
}

/// Drops segments Whisper itself flags as unlikely speech (its standard thresholds), plus known ghost phrases and
/// implausible speaking rates. An empty `segments` behaves like Swift's `nil`.
pub fn filter_whisper(text: &str, segments: &[WhisperSegment], seconds: f64) -> String {
    let mut out: Vec<String> = Vec::new();
    if !segments.is_empty() {
        for seg in segments {
            let cleaned = strip_annotations(&seg.text);
            let t = trim_hspace(&cleaned);
            if t.is_empty() {
                continue;
            }
            if seg.no_speech_prob > 0.6 && seg.avg_logprob < -0.7 {
                continue; // Whisper's own "probably not speech" rule
            }
            if seg.no_speech_prob > 0.5 && seg.avg_logprob < -0.5 {
                continue; // the same, a notch stricter: noise under a vocabulary prompt scores around (0.7, -0.63)
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
    } else {
        let t = strip_annotations(text);
        if !t.is_empty() && !ghost(&t) {
            out = vec![t];
        }
    }
    let joined = collapse_repeats(&SPACES.replace_all(&out.join(" "), " "));
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
    fn annotations() {
        assert_eq!(strip_annotations("[BLANK_AUDIO]"), "");
        assert_eq!(strip_annotations(" (music) ♪ la la ♪ "), "");
        assert_eq!(strip_annotations("Hi *sighs* there [laughs]."), "Hi there .");
        assert_eq!(filter_whisper(" [Music] Okay, let's start.", &[], 2.0), "Okay, let's start.");
    }

    #[test]
    fn noise_under_a_prompt() {
        let mut s = seg(" www.mesmerism.info", 0.6977, -0.6314);
        assert_eq!(filter_whisper("x", &[s.clone()], 5.0), "");
        s.no_speech_prob = 0.0; // confident, but still only a web address
        assert_eq!(filter_whisper("x", &[s], 5.0), "");
        assert_eq!(filter_whisper(" amara.org", &[], 2.0), "");
        assert_eq!(filter_whisper(" The docs are on example.com now.", &[], 3.0), "The docs are on example.com now.");
        assert_eq!(filter_whisper("x", &[seg(" Okay, so the launch moves.", 0.01, -0.2)], 3.0), "Okay, so the launch moves.");
    }

    #[test]
    fn repeats() {
        assert_eq!(collapse_repeats("we should ship it we should ship it today."), "we should ship it today.");
        assert_eq!(collapse_repeats("thank you thank you thank you thank you."), "thank you.");
        assert_eq!(collapse_repeats("the the the the the cat"), "the cat");
        assert_eq!(collapse_repeats("no, no. That's fine"), "no, no. That's fine");
        assert_eq!(collapse_repeats("very very good"), "very very good");
        assert_eq!(collapse_repeats(""), "");
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
