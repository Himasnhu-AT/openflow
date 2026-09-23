//! OWNER: team B (data). Mirrors Meeting.swift: transcript, clock, markdown export, todo parsing,
//! load_all / save / delete of meetings/<UUID>.json.

use std::path::PathBuf;

use chrono::{DateTime, Local, TimeZone, Utc};

use crate::model::{Meeting, MeetingTodo};
use crate::{paths, swift_json};

impl Meeting {
    pub fn new(title: String, app: Option<String>, start: chrono::DateTime<chrono::Utc>) -> Self {
        Meeting {
            id: uuid::Uuid::new_v4(),
            title,
            title_auto: true,
            todos: vec![],
            app,
            start,
            end: None,
            segments: vec![],
            summary: String::new(),
            notes: String::new(),
        }
    }
    pub fn is_live(&self) -> bool {
        self.end.is_none()
    }
    pub fn words(&self) -> usize {
        self.segments.iter().map(|s| s.text.split(' ').filter(|w| !w.is_empty()).count()).sum()
    }
    /// Seconds from start to end (or to `now` while live).
    pub fn duration_at(&self, now: DateTime<Utc>) -> f64 {
        let end = self.end.unwrap_or(now);
        (end - self.start).num_microseconds().map(|u| u as f64 / 1e6).unwrap_or(0.0)
    }
    pub fn duration(&self) -> f64 {
        self.duration_at(Utc::now())
    }
    /// "[M:SS] Speaker: text" lines, sorted by time.
    pub fn transcript(&self) -> String {
        let mut segs: Vec<_> = self.segments.iter().collect();
        segs.sort_by(|a, b| a.time.total_cmp(&b.time));
        segs.iter().map(|s| format!("[{}] {}: {}", clock(s.time), s.speaker, s.text)).collect::<Vec<_>>().join("\n")
    }
    /// `app_name` resolves a bundle id to a display name (platform-provided).
    pub fn markdown(&self, app_name: &dyn Fn(&str) -> String) -> String {
        self.markdown_in(app_name, &Local, Utc::now())
    }
    /// `markdown` with an explicit time zone and "now" (for live meetings), for tests.
    pub fn markdown_in<Tz: TimeZone>(&self, app_name: &dyn Fn(&str) -> String, tz: &Tz, now: DateTime<Utc>) -> String
    where
        Tz::Offset: std::fmt::Display,
    {
        let minutes = (self.duration_at(now) / 60.0) as i64;
        let mut md = format!("# {}\n\n{} · {} min", self.title, medium_date_short_time(&self.start, tz), minutes);
        if let Some(app) = &self.app {
            md += &format!(" · {}", app_name(app));
        }
        if !self.summary.is_empty() {
            md += &format!("\n\n{}", self.summary);
        }
        if !self.todos.is_empty() {
            md += "\n\n## To-dos\n\n";
            md += &self
                .todos
                .iter()
                .map(|t| format!("- [{}] {}", if t.done { "x" } else { " " }, t.text))
                .collect::<Vec<_>>()
                .join("\n");
        }
        if !self.notes.is_empty() {
            md += &format!("\n\n## My notes\n\n{}", self.notes);
        }
        md += &format!("\n\n## Transcript\n\n{}\n", self.transcript());
        md
    }
    /// Parses "## Your to-dos" from the summary, keeping `done` for unchanged items.
    pub fn refresh_todos(&mut self, summary: &str) {
        let Some(at) = summary.find("## Your to-dos") else { return };
        let rest = &summary[at + "## Your to-dos".len()..];
        let items: Vec<String> = rest
            .split('\n')
            .filter(|l| !l.is_empty()) // Swift's split drops empty pieces
            .take_while(|l| !l.starts_with("## "))
            .map(|l| l.trim())
            .filter(|l| l.starts_with('-'))
            .map(|l| l[1..].trim().to_string())
            .filter(|t| !t.is_empty() && t.to_lowercase() != "none")
            .collect();
        let old = std::mem::take(&mut self.todos);
        self.todos = items
            .into_iter()
            .map(|text| {
                let done = old.iter().find(|t| t.text == text).map(|t| t.done).unwrap_or(false);
                MeetingTodo { id: uuid::Uuid::new_v4(), text, done }
            })
            .collect();
    }
    /// `meetings/<UUID>.json` (uppercase, as Swift's `uuidString`).
    pub fn file_path(&self) -> PathBuf {
        paths::meetings_dir().join(format!("{}.json", self.id.hyphenated().to_string().to_uppercase()))
    }
    /// Every decodable `meetings/*.json`, newest first. Undecodable files are skipped.
    pub fn load_all() -> Vec<Meeting> {
        let Ok(entries) = std::fs::read_dir(paths::meetings_dir()) else { return vec![] };
        let mut out: Vec<Meeting> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .filter_map(|p| std::fs::read(&p).ok())
            .filter_map(|b| Meeting::from_json(&b).ok())
            .collect();
        out.sort_by_key(|m| std::cmp::Reverse(m.start));
        out
    }
    /// Decodes like Swift's `init(from:)`: optional keys may be missing or `null`.
    pub fn from_json(bytes: &[u8]) -> anyhow::Result<Meeting> {
        let mut v: serde_json::Value = serde_json::from_slice(bytes)?;
        swift_json::strip_top_level_nulls(&mut v);
        Ok(serde_json::from_value(v)?)
    }
    /// Pretty-printed, sorted keys (Swift `JSONEncoder` format).
    pub fn to_json(&self) -> anyhow::Result<String> {
        swift_json::to_pretty(self)
    }
    pub fn save(&self) -> anyhow::Result<()> {
        std::fs::create_dir_all(paths::meetings_dir())?;
        std::fs::write(self.file_path(), self.to_json()?)?;
        Ok(())
    }
    pub fn delete_file(&self) {
        let _ = std::fs::remove_file(self.file_path());
    }
}

/// `DateFormatter` `.medium` date + `.short` time in en_US: "Sep 23, 2026 at 10:14 PM" (the space before AM/PM is
/// U+202F, as current ICU prints it).
pub fn medium_date_short_time<Tz: TimeZone>(d: &DateTime<Utc>, tz: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    d.with_timezone(tz).format("%b %-d, %Y at %-I:%M\u{202F}%p").to_string()
}

/// "M:SS" or "H:MM:SS".
pub fn clock(seconds: f64) -> String {
    let s = seconds as i64; // Int(t): truncates toward zero
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apple_date::from_reference;
    use crate::config_io::test_support::DataDir;
    use crate::model::MeetingSegment;
    use uuid::Uuid;

    const SWIFT: &str = include_str!("../testdata/swift/meeting.json");
    const SWIFT_LIVE: &str = include_str!("../testdata/swift/meeting_live.json");

    #[test]
    fn clock_formats() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(5.9), "0:05");
        assert_eq!(clock(59.9), "0:59");
        assert_eq!(clock(754.0), "12:34");
        assert_eq!(clock(3600.0), "1:00:00");
        assert_eq!(clock(3723.0), "1:02:03");
        assert_eq!(clock(36000.5), "10:00:00");
        assert_eq!(clock(-0.5), "0:00");
    }

    #[test]
    fn swift_file_round_trips_byte_exact() {
        let m = Meeting::from_json(SWIFT.as_bytes()).unwrap();
        assert_eq!(m.id, Uuid::parse_str("0A1B2C3D-4E5F-4061-8293-A4B5C6D7E8F9").unwrap());
        assert_eq!(m.start, from_reference(780_000_000.25));
        assert_eq!(m.end, Some(from_reference(780_003_725.0)));
        assert_eq!(m.segments.len(), 2);
        assert!(m.todos[0].done);
        assert_eq!(m.to_json().unwrap(), SWIFT);
        let live = Meeting::from_json(SWIFT_LIVE.as_bytes()).unwrap();
        assert!(live.is_live() && live.app.is_none() && live.title_auto);
        assert_eq!(live.to_json().unwrap(), SWIFT_LIVE);
    }

    #[test]
    fn decodes_minimal_and_null_fields() {
        let m = Meeting::from_json(
            br#"{"id":"0a1b2c3d-4e5f-4061-8293-a4b5c6d7e8f9","title":"T","start":1.5,"summary":null,"end":null}"#,
        )
        .unwrap();
        assert!(m.title_auto && m.todos.is_empty() && m.segments.is_empty() && m.summary.is_empty());
        assert!(m.is_live());
        assert!(m.to_json().unwrap().contains("\"id\" : \"0A1B2C3D-4E5F-4061-8293-A4B5C6D7E8F9\""));
        assert!(Meeting::from_json(br#"{"title":"T","start":1}"#).is_err());
        assert!(Meeting::from_json(br#"{"id":"0A1B2C3D-4E5F-4061-8293-A4B5C6D7E8F9","start":1}"#).is_err());
    }

    #[test]
    fn transcript_and_markdown_match_swift() {
        let m = Meeting::from_json(SWIFT.as_bytes()).unwrap();
        assert_eq!(m.transcript(), "[0:05] You: First one\n[1:02:05] Them: Second");
        assert_eq!(m.words(), 3);
        let md = m.markdown_in(&|_| "Zoom".to_string(), &Utc, Utc::now());
        // Swift (en_US, UTC) prints the same text; see testdata/swift/generate.swift.txt.
        assert_eq!(
            md,
            "# Weekly sync\n\nSep 19, 2025 at 6:40\u{202F}PM · 62 min · Zoom\n\n## Summary\nStuff\n\n## To-dos\n\n\
             - [x] Send deck\n\n## My notes\n\nmine\n\n## Transcript\n\n[0:05] You: First one\n[1:02:05] Them: Second\n"
        );
        let live = Meeting::from_json(SWIFT_LIVE.as_bytes()).unwrap();
        let now = live.start + chrono::Duration::seconds(119);
        assert_eq!(
            live.markdown_in(&|_| unreachable!(), &Utc, now),
            "# Live\n\nJan 1, 2001 at 12:00\u{202F}AM · 1 min\n\n## Transcript\n\n\n"
        );
    }

    #[test]
    fn refresh_todos_parses_section() {
        let mut m = Meeting::new("x".into(), None, Utc::now());
        m.refresh_todos("no section here");
        assert!(m.todos.is_empty());
        let summary = "## Summary\n- not a todo\n## Your to-dos\n\n- Send deck\n  -   Book room  \n* star\n-\n- None\n\
                       text\n## Their to-dos\n- other";
        m.refresh_todos(summary);
        let texts: Vec<_> = m.todos.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, ["Send deck", "Book room"]);
        m.todos[1].done = true;
        let book_id = m.todos[1].id;
        m.refresh_todos("## Your to-dos\n- Book room\n- New one");
        assert_eq!(m.todos.len(), 2);
        assert_eq!((m.todos[0].text.as_str(), m.todos[0].done), ("Book room", true));
        assert_ne!(m.todos[0].id, book_id); // Swift makes fresh ids
        assert!(!m.todos[1].done);
        m.refresh_todos("## Your to-dos\n- none");
        assert!(m.todos.is_empty());
    }

    #[test]
    fn save_load_delete() {
        let dir = DataDir::new();
        assert!(Meeting::load_all().is_empty());
        let mut a = Meeting::new("A".into(), Some("us.zoom.xos".into()), from_reference(100.0));
        a.segments.push(MeetingSegment { id: Uuid::new_v4(), time: 1.0, speaker: "You".into(), text: "hi".into() });
        let b = Meeting::new("B".into(), None, from_reference(200.0));
        a.save().unwrap();
        b.save().unwrap();
        std::fs::write(dir.path().join("meetings/broken.json"), "{}").unwrap();
        std::fs::write(dir.path().join("meetings/notes.txt"), "x").unwrap();
        let name = a.file_path().file_name().unwrap().to_str().unwrap().to_string();
        assert_eq!(name, format!("{}.json", a.id.to_string().to_uppercase()));
        let all = Meeting::load_all();
        assert_eq!(all.iter().map(|m| m.title.as_str()).collect::<Vec<_>>(), ["B", "A"]);
        assert_eq!(all[1], a);
        a.delete_file();
        assert_eq!(Meeting::load_all().len(), 1);
    }
}
