//! Names for far-side lines. The tap hears everyone on the call as one "Them"; the call app knows who is talking.
//! Evidence arrives as observations on the meeting clock (Zoom's Accessibility tree, the Google Meet browser
//! extension) and, when Meet's captions are on, as caption text with its speaker's name.
//!
//! Adapted from Meeting-Assistant's speaker attribution (MIT): a line gets the name that was the active speaker for
//! most of its audio interval, only with a clear margin; anything ambiguous stays "Them". Evidence is never read at
//! the moment the line finishes transcribing, which would name whoever talks next.

use std::collections::{HashMap, VecDeque};

/// Detection lags speech: an observation made at `t` describes audio from about `t - LAG`.
const LAG: f64 = 0.3;
/// An observation stops counting this long after it was made, unless refreshed.
const STALE: f64 = 10.0;
/// Minimum overlap (seconds) and margin over the runner-up for a timeline name.
const MIN_OVERLAP: f64 = 0.2;
const MARGIN: f64 = 1.25;
/// Evidence kept (seconds); lines are attributed within a minute or so of being spoken.
const MEMORY: f64 = 180.0;

#[derive(Debug, Clone)]
struct Observation {
    at: f64,
    /// None: nobody (remote) is speaking.
    name: Option<String>,
}

#[derive(Debug, Clone)]
struct Caption {
    at: f64,
    name: String,
    text: String,
}

#[derive(Debug, Default)]
pub struct SpeakerTimeline {
    observations: VecDeque<Observation>,
    captions: VecDeque<Caption>,
    self_names: Vec<String>,
    /// Everyone seen on the call, in order of appearance.
    pub participants: Vec<String>,
}

fn key(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

fn words(t: &str) -> Vec<String> {
    t.split_whitespace()
        .map(|w| w.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect::<String>())
        .filter(|w| !w.is_empty())
        .collect()
}

/// A plausible display name: 2–40 characters, 1–5 words, with a letter.
pub fn valid_name(n: &str) -> bool {
    let n = n.trim();
    let chars = n.chars().count();
    (2..=40).contains(&chars) && n.chars().any(char::is_alphabetic) && n.split_whitespace().count() <= 5
}

impl SpeakerTimeline {
    pub fn is_self(&self, name: &str) -> bool {
        let k = key(name);
        k == "you" || k == "me" || self.self_names.iter().any(|s| key(s) == k)
    }

    /// The local user's own name in the call app: their tile never names a far-side line.
    pub fn set_self(&mut self, name: &str) {
        if valid_name(name) && !self.is_self(name) {
            self.self_names.push(name.trim().to_string());
        }
    }

    pub fn saw_participant(&mut self, name: &str) {
        let name = name.trim();
        if valid_name(name) && !self.is_self(name) && !self.participants.iter().any(|p| key(p) == key(name)) {
            self.participants.push(name.to_string());
        }
    }

    /// `name` is speaking at `at` (None: no remote speaker). Repeated identical observations extend the interval.
    pub fn observe(&mut self, at: f64, name: Option<&str>) {
        let name = name.map(str::trim).filter(|n| valid_name(n) && !self.is_self(n)).map(str::to_string);
        if let Some(n) = &name {
            self.saw_participant(n);
        }
        self.observations.push_back(Observation { at, name });
        self.prune(at);
    }

    /// A caption block from the call app: `name` said `text`, last updated at `at`. Blocks grow while the person
    /// talks; the latest version of a block replaces earlier ones with the same name that it extends.
    pub fn caption(&mut self, at: f64, name: &str, text: &str) {
        let name = name.trim();
        if !valid_name(name) || self.is_self(name) || text.trim().is_empty() {
            return;
        }
        self.saw_participant(name);
        if let Some(last) = self.captions.iter_mut().rev().find(|c| key(&c.name) == key(name) && at - c.at < 30.0) {
            if text.starts_with(last.text.trim_end_matches(['.', '?', '!'])) {
                last.text = text.to_string();
                last.at = at;
                return;
            }
        }
        self.captions.push_back(Caption { at, name: name.to_string(), text: text.to_string() });
        self.prune(at);
    }

    fn prune(&mut self, now: f64) {
        while self.observations.front().is_some_and(|o| o.at < now - MEMORY) {
            self.observations.pop_front();
        }
        while self.captions.front().is_some_and(|c| c.at < now - MEMORY) {
            self.captions.pop_front();
        }
    }

    /// The far-side speaker of a line with this audio interval and text, or None when the evidence is missing
    /// or ambiguous.
    pub fn attribute(&self, start: f64, end: f64, text: &str) -> Option<String> {
        self.by_caption(start, end, text).or_else(|| self.by_timeline(start, end))
    }

    /// Captions carry the words: the name whose captions around this time share the most words with the line.
    fn by_caption(&self, start: f64, end: f64, text: &str) -> Option<String> {
        let mine = words(text);
        if mine.len() < 2 {
            return None;
        }
        let mut score: HashMap<String, (usize, String)> = HashMap::new();
        for c in self.captions.iter().filter(|c| c.at >= start - 3.0 && c.at <= end + 8.0) {
            let theirs: std::collections::HashSet<String> = words(&c.text).into_iter().collect();
            let hits = mine.iter().filter(|w| theirs.contains(*w)).count();
            let e = score.entry(key(&c.name)).or_insert((0, c.name.clone()));
            e.0 = e.0.max(hits);
        }
        let mut ranked: Vec<_> = score.into_values().collect();
        ranked.sort_by_key(|r| std::cmp::Reverse(r.0));
        let (best, name) = ranked.first()?.clone();
        let runner = ranked.get(1).map(|r| r.0).unwrap_or(0);
        let enough = best >= 2 && best as f64 >= mine.len() as f64 * 0.4;
        (enough && best as f64 >= runner as f64 * 1.5 && best > runner).then_some(name)
    }

    fn by_timeline(&self, start: f64, end: f64) -> Option<String> {
        let mut overlap: HashMap<String, (f64, String)> = HashMap::new();
        let obs: Vec<&Observation> = self.observations.iter().collect();
        for (i, o) in obs.iter().enumerate() {
            let Some(name) = &o.name else { continue };
            let from = o.at - LAG;
            let until = obs.get(i + 1).map(|n| n.at - LAG).unwrap_or(f64::MAX).min(o.at + STALE);
            let shared = until.min(end) - from.max(start);
            if shared > 0.0 {
                overlap.entry(key(name)).or_insert((0.0, name.clone())).0 += shared;
            }
        }
        let mut ranked: Vec<_> = overlap.into_values().collect();
        ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
        let (best, name) = ranked.first()?.clone();
        let runner = ranked.get(1).map(|r| r.0).unwrap_or(0.0);
        (best >= MIN_OVERLAP && best >= runner * MARGIN).then_some(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dominant_speaker_over_the_interval() {
        let mut t = SpeakerTimeline::default();
        t.observe(10.0, Some("Priya Shah"));
        t.observe(14.0, Some("Rahul"));
        t.observe(15.0, Some("Rahul"));
        t.observe(20.0, None);
        assert_eq!(t.attribute(10.0, 13.5, "anything").as_deref(), Some("Priya Shah"));
        assert_eq!(t.attribute(14.0, 19.0, "anything").as_deref(), Some("Rahul"));
        // Half and half: no clear winner.
        assert_eq!(t.attribute(11.4, 16.0, "anything"), None);
        // Nobody speaking, or no evidence at all.
        assert_eq!(t.attribute(21.0, 25.0, "anything"), None);
        assert_eq!(SpeakerTimeline::default().attribute(0.0, 5.0, "x"), None);
        assert_eq!(t.participants, ["Priya Shah", "Rahul"]);
    }

    #[test]
    fn stale_evidence_stops_counting() {
        let mut t = SpeakerTimeline::default();
        t.observe(0.0, Some("Priya"));
        assert_eq!(t.attribute(30.0, 35.0, "x"), None);
        assert_eq!(t.attribute(2.0, 5.0, "x").as_deref(), Some("Priya"));
    }

    #[test]
    fn self_is_never_a_far_side_name() {
        let mut t = SpeakerTimeline::default();
        t.set_self("Himanshu");
        t.observe(0.0, Some("Himanshu"));
        t.observe(0.5, Some("You"));
        assert_eq!(t.attribute(0.0, 3.0, "x"), None);
        assert!(t.participants.is_empty());
    }

    #[test]
    fn captions_beat_the_timeline() {
        let mut t = SpeakerTimeline::default();
        t.observe(9.0, Some("Rahul")); // the tile highlight lagged
        t.caption(10.0, "Priya", "So the launch");
        t.caption(12.0, "Priya", "So the launch moves to Thursday.");
        t.caption(13.0, "Rahul", "Okay.");
        assert_eq!(t.attribute(10.0, 13.0, "So the launch moves to Thursday.").as_deref(), Some("Priya"));
        // The growing block was merged, not duplicated.
        assert_eq!(t.captions.len(), 2);
        // Words nobody captioned fall back to the timeline.
        assert_eq!(t.attribute(10.0, 13.0, "completely different words here").as_deref(), Some("Rahul"));
    }

    #[test]
    fn names_are_checked() {
        assert!(valid_name("Priya Shah"));
        assert!(!valid_name("x"));
        assert!(!valid_name("one two three four five six"));
        assert!(!valid_name("12345"));
    }
}
