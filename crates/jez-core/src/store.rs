//! OWNER: team B (data). Mirrors Store.swift: history (cap 200), stats, corrections, clips, meetings.
//! Plain data + persistence; the Tauri layer wraps it in a Mutex and emits snapshots.
//!
//! Files: `history.json` and `stats.json` are written like Swift's plain `JSONEncoder()` (compact; keys sorted
//! here for stability), `config.json` and `meetings/*.json` pretty-printed with sorted keys.

use std::fs;

use chrono::{DateTime, Local, TimeZone, Utc};
use uuid::Uuid;

use crate::model::{Dictation, Meeting, MeetingSegment, Stats};
use crate::traits::Examples;
use crate::{paths, swift_json, text_rules, Config};

pub const HISTORY_LIMIT: usize = 200;
/// `learnedSignals` keeps the most recent 20.
pub const LEARNED_SIGNALS_LIMIT: usize = 20;

pub struct Store {
    pub config: Config,
    pub history: Vec<Dictation>,
    pub stats: Stats,
    pub meetings: Vec<Meeting>,
    pub live_meeting_id: Option<Uuid>,
}

impl Stats {
    /// "yyyy-MM-dd" of `d` in the local time zone.
    pub fn key(d: &DateTime<Utc>) -> String {
        Self::key_in(d, &Local)
    }
    pub fn key_in<Tz: TimeZone>(d: &DateTime<Utc>, tz: &Tz) -> String
    where
        Tz::Offset: std::fmt::Display,
    {
        d.with_timezone(tz).format("%Y-%m-%d").to_string()
    }
    pub fn words_per_minute(&self) -> u64 {
        if self.seconds > 0.0 {
            (self.words as f64 / (self.seconds / 60.0)) as u64
        } else {
            0
        }
    }
    /// Consecutive active days ending today, or yesterday when today has no dictation yet.
    pub fn streak(&self) -> u32 {
        self.streak_at(Utc::now(), &Local)
    }
    /// Swift steps back 86 400 s at a time (not calendar days); kept as is.
    pub fn streak_at<Tz: TimeZone>(&self, now: DateTime<Utc>, tz: &Tz) -> u32
    where
        Tz::Offset: std::fmt::Display,
    {
        let days: std::collections::HashSet<&str> = self.active_days.iter().map(String::as_str).collect();
        let day_s = chrono::Duration::seconds(86_400);
        let mut day = now;
        if !days.contains(Self::key_in(&day, tz).as_str()) {
            day -= day_s;
        }
        let mut n = 0;
        while days.contains(Self::key_in(&day, tz).as_str()) {
            n += 1;
            day -= day_s;
        }
        n
    }
}

/// Decodes history.json: `[Dictation]`, or the plain `[String]` of early builds (migrated with date = now,
/// tone "" and 0 s). Anything else is an empty history.
pub fn parse_history(bytes: &[u8]) -> Vec<Dictation> {
    if let Ok(h) = serde_json::from_slice::<Vec<Dictation>>(bytes) {
        return h;
    }
    if let Ok(old) = serde_json::from_slice::<Vec<String>>(bytes) {
        let now = Utc::now();
        return old
            .into_iter()
            .map(|t| Dictation {
                id: Uuid::new_v4(),
                date: now,
                text: t.clone(),
                raw: t,
                app: None,
                tone: String::new(),
                seconds: 0.0,
                clip: None,
                correction: None,
            })
            .collect();
    }
    vec![]
}

impl Store {
    /// Loads everything from the data dir (config .bak fallback, legacy string-history migration).
    pub fn load() -> Store {
        let config = Config::load().unwrap_or_else(|e| {
            tracing::error!(target: "store", "Config load failed: {e:#}");
            Config::default()
        });
        let history = fs::read(paths::history_file()).map(|b| parse_history(&b)).unwrap_or_default();
        let stats = fs::read(paths::stats_file())
            .ok()
            .and_then(|b| serde_json::from_slice::<Stats>(&b).ok())
            .unwrap_or_default();
        Store { config, history, stats, meetings: Meeting::load_all(), live_meeting_id: None }
    }

    pub fn save_config(&self) {
        if let Err(e) = self.config.save() {
            tracing::error!(target: "store", "Config save failed: {e:#}");
        }
    }

    pub fn remember(&mut self, d: Dictation) {
        self.stats.words += d.words() as u64;
        self.stats.seconds += d.seconds;
        self.stats.dictations += 1;
        let day = Stats::key(&d.date);
        if !self.stats.active_days.contains(&day) {
            self.stats.active_days.push(day);
        }
        *self.stats.per_app.entry(d.app.clone().unwrap_or_else(|| "unknown".into())).or_default() += 1;
        *self.stats.per_tone.entry(d.tone.clone()).or_default() += 1;
        self.history.insert(0, d);
        self.history.truncate(HISTORY_LIMIT);
        self.persist_history();
        self.persist_stats();
    }

    /// Removes the entry and its clip file.
    pub fn delete(&mut self, id: Uuid) {
        let Some(i) = self.history.iter().position(|d| d.id == id) else { return };
        let d = self.history.remove(i);
        if let Some(clip) = &d.clip {
            let _ = fs::remove_file(paths::clips_dir().join(clip));
        }
        self.persist_history();
    }

    /// Deletes history.json and the whole clips/ folder. Stats are kept.
    pub fn clear_history(&mut self) {
        self.history.clear();
        let _ = fs::remove_file(paths::history_file());
        let _ = fs::remove_dir_all(paths::clips_dir());
    }

    /// Empty/unchanged text clears the correction; otherwise learns signals and names into the config.
    pub fn set_correction(&mut self, id: Uuid, text: &str) {
        let Some(i) = self.history.iter().position(|d| d.id == id) else { return };
        let t = text.trim();
        let changed = !t.is_empty() && t != self.history[i].text;
        self.history[i].correction = changed.then(|| t.to_string());
        self.persist_history();
        if !changed {
            return;
        }
        // A corrected name becomes a dictionary entry with what was heard as its sound-alike.
        let raw = self.history[i].raw.clone();
        let signals: Vec<String> = text_rules::learn_signals(&raw, t)
            .into_iter()
            .filter(|s| !self.config.learned_signals.contains(s))
            .collect();
        if !signals.is_empty() {
            let mut all = std::mem::take(&mut self.config.learned_signals);
            all.extend(signals.iter().cloned());
            let skip = all.len().saturating_sub(LEARNED_SIGNALS_LIMIT);
            self.config.learned_signals = all.split_off(skip);
            self.save_config();
            tracing::info!(target: "store", "Learned correction phrases: {}", signals.join(", "));
        }
        let learned = text_rules::learn_names(&raw, t);
        if !learned.is_empty() {
            self.config.dictionary = text_rules::merge_dictionary(&self.config.dictionary, &learned);
            self.save_config();
            let pairs: Vec<String> = learned.iter().map(|(term, alias)| format!("{term} <- {alias}")).collect();
            tracing::info!(target: "store", "Learned from correction: {}", pairs.join(", "));
        }
    }

    /// Latest corrections for a tone, oldest first, as (what was heard, what the user wanted). Swift default: 5.
    pub fn corrections(&self, tone: &str, limit: usize) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = self
            .history
            .iter()
            .filter(|d| d.tone == tone)
            .filter_map(|d| d.correction.as_ref().map(|c| (d.raw.clone(), c.clone())))
            .take(limit)
            .collect();
        out.reverse();
        out
    }

    /// Latest corrected (raw, correction) pairs per tone, `limit` each (Swift: 3), oldest first.
    pub fn all_corrections(&self, limit: usize) -> Examples {
        self.config
            .tones
            .keys()
            .filter_map(|name| {
                let c = self.corrections(name, limit);
                (!c.is_empty()).then(|| (name.clone(), c))
            })
            .collect()
    }

    pub fn correction_count(&self) -> usize {
        self.history.iter().filter(|d| d.correction.is_some()).count()
    }

    pub fn has_clip(&self, d: &Dictation) -> bool {
        d.clip.as_ref().is_some_and(|c| paths::clips_dir().join(c).exists())
    }

    pub fn words_per_minute(&self) -> u64 {
        self.stats.words_per_minute()
    }

    /// Consecutive active days ending today or yesterday.
    pub fn streak(&self) -> u32 {
        self.stats.streak()
    }

    /// Sets the polish model and saves. Returns true when it changed (the caller restarts the polisher).
    pub fn set_model(&mut self, file: &str) -> bool {
        if file == self.config.model {
            return false;
        }
        self.config.model = file.to_string();
        self.save_config();
        true
    }

    /// Sets the Parakeet version and saves. Returns true when it changed (the caller reloads speech).
    pub fn set_asr_version(&mut self, v: &str) -> bool {
        if v == self.config.asr_version {
            return false;
        }
        self.config.asr_version = v.to_string();
        self.save_config();
        true
    }

    pub fn installed_models(&self) -> Vec<String> {
        crate::catalog::installed_models()
    }

    pub fn add_meeting(&mut self, m: Meeting) {
        save_meeting(&m);
        self.meetings.insert(0, m);
    }

    /// Mutates and saves the meeting file. Returns false when the id is unknown.
    pub fn update_meeting(&mut self, id: Uuid, change: impl FnOnce(&mut Meeting)) -> bool {
        let Some(m) = self.meetings.iter_mut().find(|m| m.id == id) else { return false };
        change(m);
        save_meeting(m);
        true
    }

    pub fn append_segment(&mut self, id: Uuid, segment: MeetingSegment) {
        self.update_meeting(id, |m| m.segments.push(segment));
    }

    pub fn finish_meeting(&mut self, id: Uuid) {
        self.update_meeting(id, |m| {
            if m.end.is_none() {
                m.end = Some(Utc::now());
            }
        });
        if self.live_meeting_id == Some(id) {
            self.live_meeting_id = None;
        }
    }

    pub fn delete_meeting(&mut self, id: Uuid) {
        if let Some(m) = self.meetings.iter().find(|m| m.id == id) {
            m.delete_file();
        }
        self.meetings.retain(|m| m.id != id);
    }

    fn persist_history(&self) {
        write_compact(&paths::history_file(), &self.history);
    }

    fn persist_stats(&self) {
        write_compact(&paths::stats_file(), &self.stats);
    }
}

fn save_meeting(m: &Meeting) {
    if let Err(e) = m.save() {
        tracing::error!(target: "store", "Meeting save failed: {e:#}");
    }
}

fn write_compact<T: serde::Serialize>(path: &std::path::Path, value: &T) {
    let result = (|| -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, swift_json::to_compact(value)?)?;
        Ok(())
    })();
    if let Err(e) = result {
        tracing::error!(target: "store", "Writing {} failed: {e:#}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apple_date::from_reference;
    use crate::config_io::test_support::DataDir;
    use chrono::FixedOffset;

    fn dictation(text: &str, tone: &str, app: Option<&str>) -> Dictation {
        Dictation {
            id: Uuid::new_v4(),
            date: Utc::now(),
            text: text.into(),
            raw: text.to_lowercase(),
            app: app.map(Into::into),
            tone: tone.into(),
            seconds: 3.0,
            clip: None,
            correction: None,
        }
    }

    #[test]
    fn swift_history_and_stats_decode_and_reencode() {
        let h = parse_history(include_bytes!("../testdata/swift/history.json"));
        assert_eq!(h.len(), 1);
        let d = &h[0];
        assert_eq!(d.id, Uuid::parse_str("0A1B2C3D-4E5F-4061-8293-A4B5C6D7E8F1").unwrap());
        assert_eq!((d.text.as_str(), d.app.as_deref(), d.clip.as_deref()), ("Hi / there", None, Some("X.wav")));
        // Same members and values as Swift wrote (Swift's key order is unspecified; ours is sorted).
        assert_eq!(
            swift_json::to_compact(&h).unwrap(),
            r#"[{"clip":"X.wav","date":780000000.123456,"id":"0A1B2C3D-4E5F-4061-8293-A4B5C6D7E8F1","raw":"hi there","seconds":2.5,"text":"Hi \/ there","tone":"casual"}]"#
        );
        let s: Stats = serde_json::from_slice(include_bytes!("../testdata/swift/stats.json")).unwrap();
        assert_eq!((s.words, s.seconds, s.dictations), (3, 2.0, 1));
        assert_eq!(s.per_app.get("unknown"), Some(&1));
        assert_eq!(
            swift_json::to_compact(&s).unwrap(),
            r#"{"activeDays":["2026-09-23"],"dictations":1,"perApp":{"com.a":2,"unknown":1},"perTone":{"casual":1},"seconds":2,"words":3}"#
        );
        let e: Stats = serde_json::from_slice(include_bytes!("../testdata/swift/stats_empty.json")).unwrap();
        assert_eq!(e, Stats::default());
    }

    #[test]
    fn legacy_string_history_migrates() {
        let h = parse_history(br#"["hello world", "second"]"#);
        assert_eq!(h.len(), 2);
        assert_eq!((h[0].text.as_str(), h[0].raw.as_str(), h[0].tone.as_str()), ("hello world", "hello world", ""));
        assert_eq!(h[1].seconds, 0.0);
        assert!(parse_history(b"{\"x\":1}").is_empty());
        assert!(parse_history(b"garbage").is_empty());
    }

    #[test]
    fn load_uses_legacy_history_and_missing_files() {
        let dir = DataDir::new();
        let s = Store::load();
        assert!(s.history.is_empty() && s.meetings.is_empty() && s.live_meeting_id.is_none());
        assert_eq!(s.stats, Stats::default());
        fs::write(dir.path().join("history.json"), r#"["old one"]"#).unwrap();
        fs::write(dir.path().join("stats.json"), include_str!("../testdata/swift/stats.json")).unwrap();
        let s = Store::load();
        assert_eq!(s.history[0].text, "old one");
        assert_eq!(s.stats.words, 3);
    }

    #[test]
    fn remember_updates_stats_and_caps_history() {
        let _dir = DataDir::new();
        let mut s = Store::load();
        let mut d = dictation("one two three", "casual", None);
        d.date = from_reference(780_000_000.0);
        s.remember(d);
        s.remember(dictation("four", "professional", Some("com.apple.mail")));
        assert_eq!((s.stats.words, s.stats.dictations, s.stats.seconds), (4, 2, 6.0));
        assert_eq!(s.stats.per_app.get("unknown"), Some(&1));
        assert_eq!(s.stats.per_app.get("com.apple.mail"), Some(&1));
        assert_eq!(s.stats.per_tone.get("casual"), Some(&1));
        assert_eq!(s.stats.active_days.len(), 2);
        assert_eq!(s.stats.active_days[0], Stats::key(&from_reference(780_000_000.0)));
        assert_eq!(s.history[0].text, "four");
        for i in 0..250 {
            s.remember(dictation(&format!("w{i}"), "casual", None));
        }
        assert_eq!(s.history.len(), HISTORY_LIMIT);
        assert_eq!(s.history[0].text, "w249");
        assert_eq!(s.stats.dictations, 252);
        assert_eq!(s.stats.active_days.len(), 2);
        let reloaded = Store::load();
        // Dates only survive to Double precision (as in Swift), so compare the stored form.
        assert_eq!(swift_json::to_compact(&reloaded.history).unwrap(), swift_json::to_compact(&s.history).unwrap());
        assert_eq!(reloaded.stats, s.stats);
    }

    #[test]
    fn delete_removes_clip_and_clear_keeps_stats() {
        let dir = DataDir::new();
        let mut s = Store::load();
        let mut d = dictation("hello", "casual", None);
        let clip = format!("{}.wav", d.id.to_string().to_uppercase());
        crate::wav::write_f32(&paths::clips_dir().join(&clip), &[0.0; 16]).unwrap();
        d.clip = Some(clip.clone());
        assert!(s.has_clip(&d));
        let id = d.id;
        s.remember(d);
        s.remember(dictation("keep", "casual", None));
        s.delete(id);
        assert!(!dir.path().join("clips").join(&clip).exists());
        assert_eq!(s.history.len(), 1);
        assert_eq!(Store::load().history.len(), 1);
        s.delete(Uuid::new_v4()); // unknown id: no-op
        fs::write(paths::clips_dir().join("other.wav"), "").unwrap();
        s.clear_history();
        assert!(s.history.is_empty());
        assert!(!dir.path().join("history.json").exists());
        assert!(!dir.path().join("clips").exists());
        assert_eq!(Store::load().stats.dictations, 2);
    }

    #[test]
    fn corrections_ordering_matches_swift() {
        let _dir = DataDir::new();
        let mut s = Store::load();
        // history is newest first; corrections() takes the newest `limit` and returns them oldest first.
        for i in 0..6 {
            let mut d = dictation(&format!("t{i}"), if i % 2 == 0 { "casual" } else { "professional" }, None);
            d.raw = format!("r{i}");
            d.correction = Some(format!("c{i}"));
            s.history.insert(0, d);
        }
        s.history.insert(0, dictation("no correction", "casual", None));
        let mut other = dictation("x", "unlisted-tone", None);
        other.correction = Some("y".into());
        s.history.insert(0, other);
        assert_eq!(s.corrections("casual", 2), [("r2".into(), "c2".into()), ("r4".into(), "c4".into())]);
        assert_eq!(s.corrections("casual", 5).len(), 3);
        let all = s.all_corrections(3);
        assert_eq!(all.keys().collect::<Vec<_>>(), ["casual", "professional"]); // only tones in config.tones
        assert_eq!(all["professional"], [("r1".into(), "c1".into()), ("r3".into(), "c3".into()), ("r5".into(), "c5".into())]);
        assert_eq!(s.correction_count(), 7);
    }

    #[test]
    fn set_correction_clears_on_empty_or_unchanged() {
        let _dir = DataDir::new();
        let mut s = Store::load();
        let d = dictation("Hello there", "casual", None);
        let id = d.id;
        s.remember(d);
        s.set_correction(id, "   ");
        assert_eq!(s.history[0].correction, None);
        s.set_correction(id, " Hello there\n");
        assert_eq!(s.history[0].correction, None);
        s.set_correction(Uuid::new_v4(), "whatever"); // unknown id: no-op
        assert_eq!(s.correction_count(), 0);
    }

    #[test]
    #[ignore = "needs team A (text_rules::learn_signals / learn_names / merge_dictionary)"]
    fn set_correction_learns_into_config() {
        let _dir = DataDir::new();
        let mut s = Store::load();
        s.config.learned_signals = (0..19).map(|i| format!("sig{i}")).collect();
        let mut d = dictation("Send it to Shivon", "casual", None);
        d.raw = "send it to shivon".into();
        let id = d.id;
        s.remember(d);
        s.set_correction(id, "  Send it to Siobhan ");
        assert_eq!(s.history[0].correction.as_deref(), Some("Send it to Siobhan"));
        assert!(s.config.learned_signals.len() <= LEARNED_SIGNALS_LIMIT);
        let saved = Config::load().unwrap();
        assert_eq!(saved.dictionary, s.config.dictionary);
        assert_eq!(saved.learned_signals, s.config.learned_signals);
        // Expected from TextRules.learnNames: the corrected name with what was heard as its sound-alike.
        assert!(s.config.dictionary.iter().any(|l| l.contains("Siobhan")), "{:?}", s.config.dictionary);
    }

    #[test]
    fn wpm_and_streak() {
        let mut st = Stats { words: 300, seconds: 90.0, ..Default::default() };
        assert_eq!(st.words_per_minute(), 200);
        st.seconds = 0.0;
        assert_eq!(st.words_per_minute(), 0);

        let tz = FixedOffset::east_opt(5 * 3600 + 1800).unwrap();
        let now = tz.with_ymd_and_hms(2026, 9, 23, 0, 30, 0).unwrap().with_timezone(&Utc);
        st.active_days = vec!["2026-09-20".into(), "2026-09-21".into(), "2026-09-22".into()];
        assert_eq!(st.streak_at(now, &tz), 3); // ends yesterday
        st.active_days.push("2026-09-23".into());
        assert_eq!(st.streak_at(now, &tz), 4);
        st.active_days = vec!["2026-09-21".into()];
        assert_eq!(st.streak_at(now, &tz), 0); // gap yesterday
        assert_eq!(Stats::key_in(&now, &tz), "2026-09-23");
        assert_eq!(Stats::key_in(&now, &Utc), "2026-09-22");
    }

    #[test]
    fn meetings_lifecycle() {
        let dir = DataDir::new();
        let mut s = Store::load();
        let m = Meeting::new("Standup".into(), None, Utc::now());
        let id = m.id;
        let path = m.file_path();
        s.add_meeting(m);
        s.add_meeting(Meeting::new("Newer".into(), None, Utc::now()));
        s.live_meeting_id = Some(id);
        assert_eq!(s.meetings[1].id, id);
        assert!(path.exists());
        s.append_segment(id, MeetingSegment { id: Uuid::new_v4(), time: 2.0, speaker: "You".into(), text: "hi".into() });
        assert!(!s.update_meeting(Uuid::new_v4(), |_| panic!("unknown id")));
        let on_disk = Meeting::from_json(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(on_disk.segments.len(), 1);
        s.finish_meeting(id);
        let end = s.meetings[1].end.expect("finished");
        assert!(s.live_meeting_id.is_none());
        s.finish_meeting(id); // keeps the first end
        assert_eq!(s.meetings[1].end, Some(end));
        assert_eq!(Store::load().meetings.len(), 2);
        s.delete_meeting(id);
        assert!(!path.exists());
        assert_eq!(s.meetings.len(), 1);
        assert_eq!(fs::read_dir(dir.path().join("meetings")).unwrap().count(), 1);
    }

    #[test]
    fn set_model_and_asr_version_save() {
        let _dir = DataDir::new();
        let mut s = Store::load();
        assert!(!s.set_model(&s.config.model.clone()));
        assert!(s.set_model("Qwen3.5-9B-Q4_K_M.gguf"));
        assert!(s.set_asr_version("v3"));
        let c = Config::load().unwrap();
        assert_eq!((c.model.as_str(), c.asr_version.as_str()), ("Qwen3.5-9B-Q4_K_M.gguf", "v3"));
    }
}
