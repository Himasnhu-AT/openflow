//! OWNER: team B (data). Mirrors Meeting.swift: transcript, clock, markdown export, todo parsing,
//! load_all / save / delete of meetings/<UUID>.json.

use crate::model::Meeting;

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
    /// "[M:SS] Speaker: text" lines, sorted by time.
    pub fn transcript(&self) -> String {
        todo!("team B")
    }
    /// `app_name` resolves a bundle id to a display name (platform-provided).
    pub fn markdown(&self, _app_name: &dyn Fn(&str) -> String) -> String {
        todo!("team B")
    }
    /// Parses "## Your to-dos" from the summary, keeping `done` for unchanged items.
    pub fn refresh_todos(&mut self, _summary: &str) {
        todo!("team B")
    }
    pub fn load_all() -> Vec<Meeting> {
        todo!("team B")
    }
    pub fn save(&self) -> anyhow::Result<()> {
        todo!("team B")
    }
    pub fn delete_file(&self) {
        todo!("team B")
    }
}

/// "M:SS" or "H:MM:SS".
pub fn clock(_seconds: f64) -> String {
    todo!("team B")
}
