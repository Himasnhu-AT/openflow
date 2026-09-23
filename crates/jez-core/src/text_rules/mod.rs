//! OWNER: team A (rules). Every function of TextRules.swift. Split into submodules as you like, but keep the
//! public signatures below (other teams call them). Add more public functions freely.

/// Spoken commands: new line/paragraph, bullets, punctuation words, emails/URLs, emoji, numbers.
pub fn apply_commands(_text: &str) -> String {
    todo!("team A")
}

/// Fillers, stutters, scratch-that, restarts, swaps, repeats, article restarts.
pub fn strip_fillers(_text: &str) -> String {
    todo!("team A")
}

/// Terms only (the part before ':' / '=' / '(') of each dictionary line.
pub fn dictionary_terms(_lines: &[String]) -> Vec<String> {
    todo!("team A")
}

/// Replaces sound-alikes (and case variants of the term) with the term.
pub fn apply_dictionary(_text: &str, _lines: &[String]) -> String {
    todo!("team A")
}

/// Hans -> Hant when `traditional` and the text contains CJK.
pub fn chinese_script(_text: &str, _traditional: bool) -> String {
    todo!("team A")
}

/// Cleanup for one meeting transcript line (English).
pub fn meeting_line(_text: &str, _dictionary: &[String]) -> String {
    todo!("team A")
}

pub fn looks_english(_text: &str) -> bool {
    todo!("team A")
}

/// Capitalised names visible in `text` (window title, field text, calendar titles). Swift default limit: 15.
pub fn context_names(_text: &str, _limit: usize) -> Vec<String> {
    todo!("team A")
}

/// (term, alias) pairs learned from a correction.
pub fn learn_names(_heard: &str, _corrected: &str) -> Vec<(String, String)> {
    todo!("team A")
}

/// Phrases that turned out to mean "scratch that".
pub fn learn_signals(_heard: &str, _corrected: &str) -> Vec<String> {
    todo!("team A")
}

/// Adds learned aliases to existing lines or appends `Term: alias`.
pub fn merge_dictionary(_lines: &[String], _learned: &[(String, String)]) -> Vec<String> {
    todo!("team A")
}
