//! Speaker bleed in meetings: without headphones the microphone also hears the far side through the speakers, and
//! that audio would be transcribed a second time as "You". The system-audio tap has the clean copy, so a mic line
//! whose words were mostly just said by the far side, at the same moment, is an echo.

use std::collections::HashMap;

/// A far-side line, on the meeting clock (seconds).
#[derive(Debug, Clone)]
pub struct Heard {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// Far-side lines this close (seconds) to the mic line count as "the same moment" (device and decode latency).
const SLACK: f64 = 2.0;
/// Share of the mic line's words the far side said.
const SHARE: f64 = 0.6;

fn words(t: &str) -> Vec<String> {
    t.split_whitespace()
        .map(|w| w.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect::<String>())
        .filter(|w| !w.is_empty())
        .collect()
}

/// True when the mic line `text` (spoken between `start` and `end`) repeats what the far side said then.
/// One- and two-word lines ("yeah", "sounds good") are always kept: too little evidence, and they are exactly what
/// people say over each other.
pub fn is_echo(text: &str, start: f64, end: f64, far: &[Heard]) -> bool {
    let mine = words(text);
    if mine.len() < 3 {
        return false;
    }
    let mut pool: HashMap<String, usize> = HashMap::new();
    for h in far.iter().filter(|h| h.start <= end + SLACK && h.end >= start - SLACK) {
        for w in words(&h.text) {
            *pool.entry(w).or_default() += 1;
        }
    }
    if pool.is_empty() {
        return false;
    }
    let mut matched = 0;
    for w in &mine {
        if let Some(n) = pool.get_mut(w).filter(|n| **n > 0) {
            *n -= 1;
            matched += 1;
        }
    }
    matched as f64 >= mine.len() as f64 * SHARE
}

#[cfg(test)]
mod tests {
    use super::*;

    fn far(start: f64, end: f64, text: &str) -> Heard {
        Heard { start, end, text: text.into() }
    }

    #[test]
    fn bleed_is_caught() {
        let f = [far(10.0, 14.0, "So the launch moves to Thursday, and Priya owns the checklist.")];
        // Parakeet heard it slightly differently through the room.
        assert!(is_echo("so the launch moves to thursday and pria owns the check list", 10.3, 14.2, &f));
    }

    #[test]
    fn own_speech_is_kept() {
        let f = [far(10.0, 14.0, "So the launch moves to Thursday.")];
        assert!(!is_echo("Can we keep Wednesday instead? Marketing is ready.", 14.5, 17.0, &f));
    }

    #[test]
    fn same_words_at_another_time_are_kept() {
        let f = [far(10.0, 14.0, "the launch moves to Thursday")];
        assert!(!is_echo("the launch moves to Thursday", 40.0, 43.0, &f));
    }

    #[test]
    fn short_replies_are_kept() {
        let f = [far(10.0, 11.0, "yeah sounds good")];
        assert!(!is_echo("yeah sounds", 10.0, 11.0, &f));
        assert!(!is_echo("anything", 10.0, 11.0, &[]));
    }

    #[test]
    fn repeated_word_needs_repeated_evidence() {
        let f = [far(0.0, 3.0, "no way")];
        assert!(!is_echo("no no no no no", 0.0, 3.0, &f));
    }
}
