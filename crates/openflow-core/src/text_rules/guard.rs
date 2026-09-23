//! Guardrail after the model.

use std::collections::HashSet;

use super::util::is_alnum;

/// Lowercased words: runs of letters, marks, digits and apostrophes (TextRules.words).
pub fn words(s: &str) -> Vec<String> {
    s.to_lowercase().split(|c: char| !(is_alnum(c) || c == '\'')).filter(|w| !w.is_empty()).map(String::from).collect()
}

/// True when the model's output looks like a rewrite or hallucination rather than a cleanup of `input`.
pub fn looks_unfaithful(input: &str, output: &str, allowed: &[String]) -> bool {
    let in_words = words(input);
    let out_words = words(output);
    if in_words.len() < 6 {
        return false; // too short to judge
    }
    let ratio = out_words.len() as f64 / in_words.len() as f64;
    if !(0.35..=1.6).contains(&ratio) {
        return true;
    }
    let allowed_words: HashSet<String> = allowed.iter().flat_map(|a| words(a)).collect();
    let in_set: HashSet<&String> = in_words.iter().collect();
    let novel = out_words
        .iter()
        .filter(|w| !in_set.contains(w) && !allowed_words.contains(*w) && w.chars().count() > 3)
        .count();
    novel as f64 / out_words.len().max(1) as f64 > 0.3
}
