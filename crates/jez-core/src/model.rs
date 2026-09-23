//! Persisted records shared by the store, the UI and the orchestrator. JSON shapes match the Swift app
//! (`Store.swift`, `Meeting.swift`): camelCase keys, uppercase UUID strings, Apple-reference-date Doubles.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::apple_date;

/// Swift encodes UUIDs uppercase; serde's default is lowercase. Parsing accepts both.
pub mod upper_uuid {
    use serde::{Deserialize, Deserializer, Serializer};
    use uuid::Uuid;

    pub fn serialize<S: Serializer>(u: &Uuid, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&u.hyphenated().to_string().to_uppercase())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Uuid, D::Error> {
        let s = String::deserialize(d)?;
        Uuid::parse_str(&s).map_err(serde::de::Error::custom)
    }
}

fn new_id() -> Uuid {
    Uuid::new_v4()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dictation {
    #[serde(with = "upper_uuid", default = "new_id")]
    pub id: Uuid,
    #[serde(with = "apple_date")]
    pub date: DateTime<Utc>,
    pub text: String,
    pub raw: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    pub tone: String,
    pub seconds: f64,
    /// File name inside clips/ when the audio was kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip: Option<String>,
    /// What the user says the output should have been.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correction: Option<String>,
}

impl Dictation {
    pub fn words(&self) -> usize {
        self.text.split_whitespace().count()
    }
}

/// All-time counters, kept separately so trimming history doesn't lose them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Stats {
    pub words: u64,
    pub seconds: f64,
    pub dictations: u64,
    /// "yyyy-MM-dd", local time.
    pub active_days: Vec<String>,
    pub per_app: BTreeMap<String, u64>,
    pub per_tone: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeetingSegment {
    #[serde(with = "upper_uuid")]
    pub id: Uuid,
    /// Seconds from the meeting start.
    pub time: f64,
    /// "You" (microphone) or "Them" (system audio).
    pub speaker: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeetingTodo {
    #[serde(with = "upper_uuid")]
    pub id: Uuid,
    pub text: String,
    #[serde(default)]
    pub done: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Meeting {
    #[serde(with = "upper_uuid")]
    pub id: Uuid,
    pub title: String,
    /// True until the user edits the title.
    #[serde(default = "yes")]
    pub title_auto: bool,
    #[serde(default)]
    pub todos: Vec<MeetingTodo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(with = "apple_date")]
    pub start: DateTime<Utc>,
    #[serde(default, with = "apple_date::option", skip_serializing_if = "Option::is_none")]
    pub end: Option<DateTime<Utc>>,
    #[serde(default)]
    pub segments: Vec<MeetingSegment>,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub notes: String,
}

fn yes() -> bool {
    true
}

/// Contacts/Calendar/field names and a speech-ready flag etc. are runtime state, not persisted.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputDevice {
    pub uid: String,
    pub name: String,
    pub is_built_in: bool,
    pub is_continuity: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub bundle_id: String,
    pub name: String,
    /// PNG as a data: URL, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}
