//! Dictionary (names and jargon, with sound-alikes) and learning from corrections.

use fancy_regex::Regex;
use once_cell::sync::Lazy;

use super::util::{find_caps, replace_all, rx, trim_hspace, trim_punct, trim_punct_except};

static SEPARATOR: Lazy<Regex> = Lazy::new(|| rx(r"\s*[:=]\s*"));
static PARENS: Lazy<Regex> = Lazy::new(|| rx(r"\s*\((.*)\)\s*$"));

/// One dictionary entry: the term and its sound-alikes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictionaryEntry {
    pub term: String,
    pub aliases: Vec<String>,
}

/// A dictionary line is "Term" or "Term: sound-alike, sound-alike" (also "=" or parentheses).
pub fn parse_dictionary(lines: &[String]) -> Vec<DictionaryEntry> {
    lines
        .iter()
        .filter_map(|line| {
            let l = trim_hspace(line);
            if l.is_empty() {
                return None;
            }
            let (mut term, mut rest) = (l.to_string(), String::new());
            if let Some(r) = find_caps(&SEPARATOR, l).and_then(|c| c.get(0)) {
                term = l[..r.start()].to_string();
                rest = l[r.end()..].to_string();
            } else if let Some(r) = find_caps(&PARENS, l).and_then(|c| c.get(0)) {
                term = l[..r.start()].to_string();
                rest = l[r.start()..r.end()].trim_matches(|c| c == ' ' || c == '(' || c == ')').to_string();
            }
            let aliases = rest
                .split([',', ';'])
                .filter(|s| !s.is_empty())
                .map(|s| trim_hspace(s).to_string())
                .filter(|s| !s.is_empty())
                .collect();
            Some(DictionaryEntry { term: trim_hspace(&term).to_string(), aliases })
        })
        .collect()
}

pub fn dictionary_terms(lines: &[String]) -> Vec<String> {
    parse_dictionary(lines).into_iter().map(|e| e.term).collect()
}

/// Replaces sound-alikes with the term, and fixes the term's own capitalisation. Whole words only.
pub fn apply_dictionary(text: &str, lines: &[String]) -> String {
    let mut t = text.to_string();
    for e in parse_dictionary(lines) {
        let mut variants: Vec<&String> = std::iter::once(&e.term).chain(e.aliases.iter()).collect();
        variants.sort_by_key(|v| std::cmp::Reverse(v.chars().count()));
        for v in variants.into_iter().filter(|v| v.chars().count() >= 2) {
            // Swift swaps "\ " for [\s-]*, but NSRegularExpression.escapedPattern never escapes spaces, so a
            // space in a variant matches one literal space.
            let Ok(re) = Regex::new(&format!(r"(?i)\b{}\b", regex::escape(v))) else { continue };
            // The term is an NSRegularExpression template ($n, \x), exactly as in Swift.
            t = replace_all(&re, &t, &e.term);
        }
    }
    t
}

// MARK: Learning names

pub(crate) const COMMON_CAPITALISED: &[&str] = &[
    "i", "i'm", "i'll", "i've", "i'd", "the", "a", "an", "and", "but", "or", "so", "hi", "hey", "hello", "dear", "thanks", "thank", "cheers",
    "kind", "best", "regards", "yes", "no", "ok", "okay", "yeah", "also", "please", "sorry", "just", "well", "oh", "wow", "haha", "lol",
    "monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday", "january", "february", "march", "april", "may", "june",
    "july", "august", "september", "october", "november", "december", "today", "tomorrow", "yesterday", "morning", "afternoon", "evening",
    "this", "that", "these", "those", "there", "here", "what", "when", "where", "who", "why", "how", "it", "it's", "we", "we're", "you",
    "he", "she", "they", "my", "your", "our", "his", "her", "their", "if", "in", "on", "at", "to", "for", "of", "with", "from", "by",
    "not", "can", "could", "would", "should", "will", "do", "does", "did", "is", "are", "was", "were", "be", "been", "have", "has", "had",
    "let's", "let", "hope", "see", "send", "get", "got", "one", "two", "three", "first", "second", "third", "new", "re", "fw", "fwd",
    "window", "untitled", "all", "notes", "messages", "mail", "chrome", "safari", "google", "search", "inbox", "sent", "draft",
];

fn is_common(w: &str) -> bool {
    COMMON_CAPITALISED.contains(&w)
}

fn first_upper(w: &str) -> bool {
    w.chars().next().is_some_and(char::is_uppercase)
}

/// Capitalised words and phrases in `text` that look like names, in order of appearance ("Wispr Flow", "Zentro").
pub fn context_names(text: &str, limit: usize) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().map(|w| trim_punct_except(w, &['\'', '’', '-'])).collect();
    let lower: std::collections::HashSet<String> = words
        .iter()
        .filter(|w| w.chars().next().is_some_and(char::is_lowercase))
        .map(|w| w.to_lowercase())
        .collect();
    let is_name = |w: &str| {
        let lw = w.to_lowercase();
        w.chars().count() >= 3
            && first_upper(w)
            && w.chars().skip(1).any(char::is_alphabetic)
            && !is_common(&lw)
            && !lower.contains(&lw)
    };
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        if !is_name(words[i]) {
            i += 1;
            continue;
        }
        let mut phrase = vec![words[i]];
        let mut j = i + 1;
        while j < words.len() && phrase.len() < 3 && is_name(words[j]) {
            phrase.push(words[j]);
            j += 1;
        }
        let name = phrase.join(" ");
        if !out.iter().any(|o| o.to_lowercase() == name.to_lowercase()) {
            out.push(name);
        }
        i = j;
    }
    out.truncate(limit);
    out
}

fn norm_keep_apostrophes(w: &str) -> String {
    trim_punct_except(&w.to_lowercase(), &['\'', '’']).to_string()
}

/// LCS table over normalised words: `dp[i][j]` = LCS length of `a[i..]` and `b[j..]`.
fn lcs(a: &[String], b: &[String]) -> Vec<Vec<usize>> {
    let mut dp = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            dp[i][j] = if a[i] == b[j] { dp[i + 1][j + 1] + 1 } else { dp[i + 1][j].max(dp[i][j + 1]) };
        }
    }
    dp
}

/// What a correction teaches: (term, sound-alike) pairs where the user replaced a span of the transcript with a
/// capitalised name. "i've done zentrow" -> "i've done Zentro" gives ("Zentro", "zentrow").
pub fn learn_names(heard: &str, corrected: &str) -> Vec<(String, String)> {
    let a: Vec<&str> = heard.split_whitespace().collect();
    let b: Vec<&str> = corrected.split_whitespace().collect();
    if a.is_empty() || b.is_empty() || a.len() >= 400 || b.len() >= 400 {
        return vec![];
    }
    let na: Vec<String> = a.iter().map(|w| norm_keep_apostrophes(w)).collect();
    let nb: Vec<String> = b.iter().map(|w| norm_keep_apostrophes(w)).collect();
    let dp = lcs(&na, &nb);
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if na[i] == nb[j] {
            i += 1;
            j += 1;
            continue;
        }
        // A differing region: advance both sides until the LCS resumes.
        let (mut ai, mut bj) = (i, j);
        while ai < a.len() || bj < b.len() {
            if ai < a.len() && bj < b.len() && na[ai] == nb[bj] && dp[ai][bj] == dp[ai + 1][bj + 1] + 1 {
                break;
            }
            if ai < a.len() && dp[ai][bj] == dp[ai + 1][bj] {
                ai += 1;
                continue;
            }
            if bj < b.len() && dp[ai][bj] == dp[ai][bj + 1] {
                bj += 1;
                continue;
            }
            break;
        }
        // A name can continue with words that were heard right ("whisper flow" -> "Wispr Flow"): extend the span
        // while both sides agree and the corrected word is capitalised.
        let (mut heard_end, mut fixed_end) = (ai, bj);
        while heard_end < a.len()
            && fixed_end < b.len()
            && na[heard_end] == nb[fixed_end]
            && fixed_end - j < 3
            && first_upper(b[fixed_end])
            && !is_common(&nb[fixed_end])
        {
            heard_end += 1;
            fixed_end += 1;
        }
        let heard_span: Vec<&str> = a[i..heard_end].iter().map(|w| trim_punct(w)).filter(|w| !w.is_empty()).collect();
        let fixed_span: Vec<&str> = b[j..fixed_end]
            .iter()
            .map(|w| trim_punct_except(w, &['\'', '’', '-']))
            .filter(|w| !w.is_empty())
            .collect();
        if !heard_span.is_empty()
            && !fixed_span.is_empty()
            && heard_span.len() <= 4
            && fixed_span.len() <= 3
            && fixed_span.iter().all(|w| first_upper(w) && !is_common(&w.to_lowercase()))
        {
            let term = fixed_span.join(" ");
            let alias = heard_span.join(" ").to_lowercase();
            if term.to_lowercase() != alias && term.chars().count() >= 3 {
                out.push((term, alias));
            }
        }
        i = heard_end;
        j = fixed_end;
    }
    out
}

/// Correction phrases a correction reveals: when the user deleted a run of words that ends in a short aside, the
/// aside is how their "no wait" gets heard. Returns lowercased 1-2 word phrases.
pub fn learn_signals(heard: &str, corrected: &str) -> Vec<String> {
    let a: Vec<&str> = heard.split_whitespace().collect();
    let b: Vec<&str> = corrected.split_whitespace().collect();
    if a.len() < 4 || b.is_empty() || a.len() >= 400 || b.len() >= 400 {
        return vec![];
    }
    let na: Vec<String> = a.iter().map(|w| norm_keep_apostrophes(w)).collect();
    let nb: Vec<String> = b.iter().map(|w| norm_keep_apostrophes(w)).collect();
    let dp = lcs(&na, &nb);
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < a.len() {
        if j < b.len() && na[i] == nb[j] {
            i += 1;
            j += 1;
            continue;
        }
        if j < b.len() && dp[i][j] == dp[i][j + 1] {
            j += 1;
            continue;
        }
        // a[i] was deleted: collect the whole deleted run
        let mut k = i;
        while k < a.len() && !(j < b.len() && na[k] == nb[j] && dp[k][j] == dp[k + 1][j + 1] + 1) {
            k += 1;
        }
        let run: Vec<&String> = na[i..k].iter().filter(|w| !w.is_empty()).collect();
        // A deleted run of 3+ words whose tail is a short aside that was followed by kept text.
        if run.len() >= 3 && k < a.len() {
            for n in [2, 1] {
                if run.len() <= n {
                    continue;
                }
                let tail = run[run.len() - n..].iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ");
                if !is_common(&tail) || n == 2 {
                    out.push(tail);
                    break;
                }
            }
        }
        i = k;
    }
    out
}

/// Adds learned pairs to dictionary lines, merging sound-alikes into an existing term.
pub fn merge_dictionary(lines: &[String], learned: &[(String, String)]) -> Vec<String> {
    let mut lines = lines.to_vec();
    for (term, alias) in learned {
        let idx = lines.iter().position(|l| {
            parse_dictionary(std::slice::from_ref(l)).first().is_some_and(|e| e.term.to_lowercase() == term.to_lowercase())
        });
        if let Some(idx) = idx {
            let e = parse_dictionary(std::slice::from_ref(&lines[idx])).remove(0);
            if !e.aliases.iter().any(|a| a.to_lowercase() == *alias) {
                let mut aliases = e.aliases.clone();
                aliases.push(alias.clone());
                lines[idx] = format!("{}: {}", e.term, aliases.join(", "));
            }
        } else {
            lines.push(format!("{term}: {alias}"));
        }
    }
    lines
}
