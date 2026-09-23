//! What the user is typing into, read at the moment the hotkey goes down (FieldContext.swift).
//! Only used to interpret the dictation; never logged.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldContext {
    /// Title of the frontmost window (max 120 chars).
    pub window_title: String,
    /// Text before the cursor (last 3000 UTF-16 units).
    pub before: String,
    /// Text after the cursor (first 600 UTF-16 units).
    pub after: String,
    /// Cursor position in UTF-16 units, when the field reported a selection range.
    pub cursor: Option<usize>,
}

impl FieldContext {
    pub fn is_empty(&self) -> bool {
        self.window_title.is_empty() && self.before.is_empty() && self.after.is_empty()
    }

    /// True when the cursor sits right after a non-space character, so pasted text needs a leading space.
    pub fn needs_leading_space(&self) -> bool {
        self.before.chars().last().is_some_and(|c| !c.is_whitespace())
    }
}
