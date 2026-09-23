//! OWNER: team A (rules). Port of TextRules.swift: deterministic text rules that run around the polish model
//! (cheap cleanup before it, a sanity check after it). The public signatures below are used by other teams.

mod commands;
mod dictionary;
mod fillers;
mod guard;
mod layout;
mod meeting_rules;
pub mod selftest;
pub(crate) mod util;

#[cfg(test)]
mod tests;

pub use commands::{apply_emoji, apply_numbers, parse_number, ParsedNumber, EMOJI_NAMES};
pub use dictionary::{parse_dictionary, DictionaryEntry};
pub use fillers::{apply_repeats, apply_restarts, apply_scratch, apply_swaps, article_restart};
pub use guard::{looks_unfaithful, words};
pub use layout::{
    apply_counted_lists, apply_lists, casual, layout_greeting, layout_message, question_marks, strip_echo,
};
pub use meeting_rules::{capitalise_proper_names, contains_chinese};

/// Swift's default `limit` for `contextNames`.
pub const CONTEXT_NAMES_LIMIT: usize = 15;

/// Spoken commands: new line/paragraph, bullets, punctuation words, emails/URLs, emoji, numbers.
pub fn apply_commands(text: &str) -> String {
    commands::apply_commands(text)
}

/// Fillers, stutters, scratch-that, restarts, swaps, repeats, article restarts.
pub fn strip_fillers(text: &str) -> String {
    fillers::strip_fillers(text)
}

/// Terms only (the part before ':' / '=' / '(') of each dictionary line.
pub fn dictionary_terms(lines: &[String]) -> Vec<String> {
    dictionary::dictionary_terms(lines)
}

/// Replaces sound-alikes (and case variants of the term) with the term.
pub fn apply_dictionary(text: &str, lines: &[String]) -> String {
    dictionary::apply_dictionary(text, lines)
}

/// Hans -> Hant when `traditional` and the text contains CJK.
pub fn chinese_script(text: &str, traditional: bool) -> String {
    meeting_rules::chinese_script(text, traditional)
}

/// Cleanup for one meeting transcript line (English).
pub fn meeting_line(text: &str, dictionary: &[String]) -> String {
    meeting_rules::meeting_line(text, dictionary)
}

pub fn looks_english(text: &str) -> bool {
    meeting_rules::looks_english(text)
}

/// Capitalised names visible in `text` (window title, field text, calendar titles). Swift default limit: 15.
pub fn context_names(text: &str, limit: usize) -> Vec<String> {
    dictionary::context_names(text, limit)
}

/// (term, alias) pairs learned from a correction.
pub fn learn_names(heard: &str, corrected: &str) -> Vec<(String, String)> {
    dictionary::learn_names(heard, corrected)
}

/// Phrases that turned out to mean "scratch that".
pub fn learn_signals(heard: &str, corrected: &str) -> Vec<String> {
    dictionary::learn_signals(heard, corrected)
}

/// Adds learned aliases to existing lines or appends `Term: alias`.
pub fn merge_dictionary(lines: &[String], learned: &[(String, String)]) -> Vec<String> {
    dictionary::merge_dictionary(lines, learned)
}

/// TextRules.wordErrorRate (also exposed as `crate::wer::word_error_rate`).
pub fn word_error_rate(reference: &str, hypothesis: &str) -> f64 {
    crate::wer::word_error_rate(reference, hypothesis)
}
