//! Fillers, stutters and self-corrections (before the model, never in verbatim).

use fancy_regex::Regex;
use once_cell::sync::Lazy;

use super::util::{find_caps, replace_all, rx, trim_hspace, trim_punct};

static FILLER: Lazy<Regex> = Lazy::new(|| rx(r"(?i)(^|[\s,])(um+|uh+|uhm|erm?|hmm+|mm+)\b[,.]?\s*"));

/// "scratch that" / "forget that" / "never mind" throw away what was said just before it: the unfinished
/// sentence it sits in, or, when the transcriber already closed that sentence, the whole previous sentence.
static SCRATCH_SIGNAL: Lazy<Regex> =
    Lazy::new(|| rx(r"(?i)\b(?:actually\s+)?(?:scratch that|forget that|never ?mind)\b[,.!?]?\s*"));

fn is_terminator(c: char) -> bool {
    matches!(c, '.' | '!' | '?')
}

pub fn apply_scratch(text: &str) -> String {
    let mut t = text.to_string();
    while let Some(m) = SCRATCH_SIGNAL.find(&t).ok().flatten() {
        let chars: Vec<char> = t[..m.start()].chars().collect();
        let mut i = chars.len();
        while i > 0 && chars[i - 1].is_whitespace() {
            i -= 1;
        }
        let sentence_start = i == 0 || is_terminator(chars[i - 1]);
        if sentence_start && i > 0 {
            i -= 1; // step over the terminator to reach the sentence before
        }
        while i > 0 && !is_terminator(chars[i - 1]) {
            i -= 1; // back to the previous sentence end
        }
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        let head: String = chars[..i].iter().collect();
        t = head + &t[m.end()..];
    }
    t
}

/// "she actually he", "tuesday actually thursday", "five actually six": when the words on both sides of a
/// correction signal are the same kind of thing, keep the second one.
const SWAP_CLASSES: &[&[&str]] = &[
    &["he", "she", "they", "him", "her", "them", "his", "hers", "their", "i", "we", "you", "me", "us"],
    &["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"],
    &["january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december"],
    &["one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve"],
    &["morning", "afternoon", "evening", "tonight", "today", "tomorrow", "yesterday"],
];
static SWAP: Lazy<Regex> = Lazy::new(|| rx(r"(?i)\b([\w']+)[,.]?\s+(?:actually|no|sorry|i mean)[,.]?\s+([\w']+)\b"));

/// Swift `Int(String)`.
fn is_int(s: &str) -> bool {
    s.parse::<i64>().is_ok()
}

pub fn apply_swaps(text: &str) -> String {
    let mut t = text.to_string();
    let mut from = 0;
    // Searching a slice reproduces NSRegularExpression's default opaque, anchoring range bounds.
    while from < t.len() {
        let Some(c) = find_caps(&SWAP, &t[from..]) else { break };
        let whole = c.get(0).expect("match");
        let (start, end) = (from + whole.start(), from + whole.end());
        let x = c.get(1).expect("x").as_str().to_lowercase();
        let y = c.get(2).expect("y").as_str().to_string();
        let yl = y.to_lowercase();
        let same_class = x != yl
            && (SWAP_CLASSES.iter().any(|cl| cl.contains(&x.as_str()) && cl.contains(&yl.as_str()))
                || (is_int(&x) && is_int(&yl)));
        if same_class {
            t.replace_range(start..end, &y);
            from = start + y.len();
        } else {
            from = start + t[start..].chars().next().map_or(1, char::len_utf8);
        }
    }
    t
}

/// Restarts: the words after the signal repeat the start of the abandoned clause, so the abandoned clause goes.
static RESTART_SIGNAL: Lazy<Regex> =
    Lazy::new(|| rx(r"(?i)[,.]?\s*\b(?:sorry|no[,\s]+wait|wait[,\s]+no|i mean|correction|let me rephrase)\b[,.]?\s*"));
static WORD: Lazy<Regex> = Lazy::new(|| rx(r"[^\s]+"));
const STARTERS: &[&str] = &[
    "because", "so", "and", "but", "if", "when", "then", "can", "could", "would", "should", "i", "we", "you", "he", "she",
    "they", "it", "the", "to", "let's", "please", "send", "tell",
];

/// Byte ranges of the `[^\s]+` words in `text[lo..hi]`, as offsets into `text`.
fn word_ranges(text: &str, lo: usize, hi: usize) -> Vec<(usize, usize)> {
    WORD.find_iter(&text[lo..hi]).filter_map(Result::ok).map(|m| (lo + m.start(), lo + m.end())).collect()
}

fn norm(text: &str, r: (usize, usize)) -> String {
    trim_punct(&text[r.0..r.1].to_lowercase()).to_string()
}

pub fn apply_restarts(text: &str) -> String {
    let mut t = text.to_string();
    let mut from = 0;
    while from < t.len() {
        let Some(sig) = RESTART_SIGNAL.find(&t[from..]).ok().flatten() else { break };
        let (sig_start, after_start) = (from + sig.start(), from + sig.end());
        // Words of the same sentence before the signal (last 12), and up to 6 after it.
        let mut before = word_ranges(&t, 0, sig_start);
        if let Some(end) = before.iter().rposition(|r| {
            let w = &t[r.0..r.1];
            w.ends_with('.') || w.ends_with('!') || w.ends_with('?')
        }) {
            before = before[end + 1..].to_vec();
        }
        if before.len() > 12 {
            before = before[before.len() - 12..].to_vec();
        }
        let after: Vec<_> = word_ranges(&t, after_start, t.len()).into_iter().take(6).collect();
        let (mut best_k, mut best_len) = (usize::MAX, 0);
        for k in 0..before.len() {
            let mut len = 0;
            while k + len < before.len() && len < after.len() && norm(&t, before[k + len]) == norm(&t, after[len]) {
                len += 1;
            }
            if len > best_len {
                best_len = len;
                best_k = k;
            }
        }
        let accept = best_len >= 2 || (best_len == 1 && STARTERS.contains(&norm(&t, before[best_k]).as_str()));
        if accept {
            let start = before[best_k].0;
            t.replace_range(start..after_start, "");
            from = start;
        } else {
            from = after_start;
        }
    }
    t
}

/// Restart without a signal word: a phrase of 3+ words said again after a garbled false start. The gap must be
/// short and must not end in a conjunction.
const CONJUNCTIONS: &[&str] = &["and", "or", "but", "then", "so", "because", "if", "when"];

pub fn apply_repeats(text: &str) -> String {
    let words = word_ranges(text, 0, text.len());
    if words.len() < 6 {
        return text.to_string();
    }
    let n = |k: usize| norm(text, words[k]);
    for i in 0..(words.len() - 5) {
        for gap in 0..=4 {
            let j = i + 3 + gap;
            if j + 2 >= words.len() {
                continue;
            }
            if n(i) == n(j)
                && n(i + 1) == n(j + 1)
                && n(i + 2) == n(j + 2)
                && !CONJUNCTIONS.contains(&n(j - 1).as_str())
                && !n(i).is_empty()
            {
                let mut s = text.to_string();
                s.replace_range(words[i].0..words[j].0, "");
                return apply_repeats(&s);
            }
        }
    }
    text.to_string()
}

static SPACES: Lazy<Regex> = Lazy::new(|| rx(r"\s{2,}"));
static LEADING: Lazy<Regex> = Lazy::new(|| rx(r"^[,\s]+"));
static WORD_RUN: Lazy<regex::Regex> = Lazy::new(|| regex::Regex::new(r"\w+").expect("regex"));

/// Maximal `\w` runs (byte ranges). Every match of the backreference rules below starts at one of these.
fn runs(text: &str) -> Vec<(usize, usize)> {
    WORD_RUN.find_iter(text).map(|m| (m.start(), m.end())).collect()
}

/// ICU compares `(?i)` backreferences caselessly; fancy-regex does not, so the two rules that use one are
/// hand-coded over word runs.
fn same_caseless(a: &str, b: &str) -> bool {
    a == b || a.to_lowercase() == b.to_lowercase()
}

/// Length of `[,.]?\s+` (or `[,]?\s+` when `dots` is false) at the start of `s`, if it matches.
fn separator_len(s: &str, dots: bool) -> Option<usize> {
    let mut len = 0;
    let mut it = s.chars().peekable();
    if let Some(&c) = it.peek() {
        if c == ',' || (dots && c == '.') {
            len += 1;
            it.next();
        }
    }
    let ws: usize = it.take_while(|c| c.is_whitespace()).map(char::len_utf8).sum();
    (ws > 0).then_some(len + ws)
}

/// `(?i)\b(\w+)([,.]?\s+)\1\b(?=[\s,.!?]|$)` -> `$1`, one pass.
fn stutter_pass(text: &str) -> String {
    let rs = runs(text);
    let mut out = String::with_capacity(text.len());
    let (mut last, mut k) = (0, 0);
    while k < rs.len() {
        let (s, e) = rs[k];
        let matched = separator_len(&text[e..], true).and_then(|sep| {
            let next = *rs.get(k + 1)?;
            let ok = next.0 == e + sep
                && same_caseless(&text[s..e], &text[next.0..next.1])
                && text[next.1..].chars().next().is_none_or(|c| c.is_whitespace() || ",.!?".contains(c));
            ok.then_some(next.1)
        });
        match matched {
            Some(end) => {
                out.push_str(&text[last..e]);
                last = end;
                k += 2;
            }
            None => k += 1,
        }
    }
    out.push_str(&text[last..]);
    out
}

const ARTICLE_BLOCKERS: &[&str] = &["the", "a", "an", "of", "in", "on", "at", "by", "to", "for"];

/// "this is fair a fair bit of work" -> "this is a fair bit of work": a word restarted with its article.
/// `(?i)(^|(?<!\b(?:the|a|an|of|in|on|at|by|to|for)\s)\b)(\w+)[,]?\s+(a|an|the)\s+\2\b` -> `$1$3 $2`.
pub fn article_restart(text: &str) -> String {
    let rs = runs(text);
    let mut out = String::with_capacity(text.len());
    let (mut last, mut k) = (0, 0);
    while k + 2 < rs.len() {
        let (s, e) = rs[k];
        let blocked = s > 0 && {
            // One whitespace char before the word, and before that a whole blocker word.
            let before = &text[..s];
            let ws = before.chars().next_back().filter(|c| c.is_whitespace());
            ws.is_some_and(|c| {
                let p = s - c.len_utf8();
                k > 0 && rs[k - 1].1 == p && ARTICLE_BLOCKERS.contains(&text[rs[k - 1].0..p].to_lowercase().as_str())
            })
        };
        let (a, w2) = (rs[k + 1], rs[k + 2]);
        let article = text[a.0..a.1].to_lowercase();
        let ok = !blocked
            && separator_len(&text[e..], false).is_some_and(|sep| e + sep == a.0)
            && matches!(article.as_str(), "a" | "an" | "the")
            && text[a.1..w2.0].chars().all(char::is_whitespace)
            && a.1 < w2.0
            && same_caseless(&text[s..e], &text[w2.0..w2.1]);
        if ok {
            out.push_str(&text[last..s]);
            out.push_str(&text[a.0..a.1]);
            out.push(' ');
            out.push_str(&text[s..e]);
            last = w2.1;
            k += 3;
        } else {
            k += 1;
        }
    }
    out.push_str(&text[last..]);
    out
}

pub fn strip_fillers(text: &str) -> String {
    let mut t = apply_swaps(&apply_repeats(&apply_restarts(&apply_scratch(text))));
    t = article_restart(&t);
    t = replace_all(&FILLER, &t, "$1");
    // Run twice so "we we we" collapses fully.
    for _ in 0..2 {
        t = stutter_pass(&t);
    }
    t = replace_all(&SPACES, &t, " ");
    t = replace_all(&LEADING, &t, "");
    trim_hspace(&t).to_string()
}
