//! Meeting lines, language detection and Chinese script.

use fancy_regex::Regex;
use once_cell::sync::Lazy;

use super::commands::apply_numbers;
use super::dictionary::apply_dictionary;
use super::fillers::strip_fillers;
use super::layout::question_marks;
use super::util::{replace_all, rx, upper_first};

const ENGLISH_STOPWORDS: &[&str] = &[
    "the", "and", "is", "it", "to", "you", "i", "we", "a", "of", "in", "that", "on", "for", "so", "this", "be", "have", "are",
    "was", "with", "just", "not", "can", "but", "what", "do", "if", "they", "he", "she", "my", "me", "your", "our", "yeah",
    "okay", "ok", "like", "um", "uh", "good", "going", "get", "got", "know", "think", "let's", "then", "there", "at", "by",
    "from", "or", "will", "would", "about", "how", "when", "up", "out", "one", "all", "no", "yes", "right", "well", "see",
    "us", "them", "an", "as", "has", "had",
];

/// True when the text reads as English (enough common English words).
pub fn looks_english(text: &str) -> bool {
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower.split(|c: char| !(c.is_alphabetic() || c == '\'')).filter(|w| !w.is_empty()).collect();
    if words.is_empty() || contains_chinese(text) {
        return false;
    }
    let hits = words.iter().filter(|w| ENGLISH_STOPWORDS.contains(w)).count();
    hits as f64 / words.len() as f64 >= 0.15 || (words.len() <= 3 && hits >= 1)
}

static LOWER_I: Lazy<Regex> = Lazy::new(|| rx(r"\bi\b"));

/// Cleanup for one English meeting line: fillers out, names and numbers fixed, sentence case, end punctuation.
/// Returns "" when nothing but fillers was said.
pub fn meeting_line(text: &str, dictionary: &[String]) -> String {
    let t = strip_fillers(text);
    let t = t.trim();
    if t.is_empty() || !t.chars().any(|c| c.is_alphabetic() || c.is_numeric()) {
        return String::new();
    }
    let mut t = apply_dictionary(&apply_numbers(t), dictionary);
    t = replace_all(&LOWER_I, &t, "I");
    t = capitalise_proper_names(&t);
    t = upper_first(&t);
    if let Some(last) = t.chars().last() {
        if !".?!".contains(last) {
            t.push('.');
        }
    }
    question_marks(&t)
}

const PROPER_WORDS: &[&str] = &[
    "monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday", "january", "february", "march", "april",
    "june", "july", "august", "september", "october", "november", "december", "instagram", "youtube", "tiktok", "linkedin",
    "facebook", "twitter", "google", "apple", "microsoft", "zoom", "slack", "notion", "figma", "chatgpt", "claude",
    "australia", "sydney", "melbourne", "america", "london", "english", "chinese", "cantonese", "mandarin", "christmas",
    "easter",
];
static PROPER: Lazy<Regex> = Lazy::new(|| rx(&format!(r"(?i)\b({})\b", PROPER_WORDS.join("|"))));

fn brand_case(w: &str) -> Option<&'static str> {
    Some(match w {
        "chatgpt" => "ChatGPT",
        "linkedin" => "LinkedIn",
        "youtube" => "YouTube",
        "tiktok" => "TikTok",
        _ => return None,
    })
}

pub fn capitalise_proper_names(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for m in PROPER.find_iter(text).filter_map(Result::ok) {
        out.push_str(&text[last..m.start()]);
        let w = m.as_str();
        match brand_case(&w.to_lowercase()) {
            Some(b) => out.push_str(b),
            None => out.push_str(&upper_first(w)),
        }
        last = m.end();
    }
    out.push_str(&text[last..]);
    out
}

pub fn contains_chinese(text: &str) -> bool {
    text.chars().any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c))
}

/// Speech models write Cantonese and Mandarin in Simplified characters; convert to Traditional when asked.
/// Swift uses ICU's "Hans-Hant" transform; zhconv's zh-Hant tables are the closest offline equivalent.
pub fn chinese_script(text: &str, traditional: bool) -> String {
    if !traditional || !contains_chinese(text) {
        return text.to_string();
    }
    zhconv::zhconv(text, zhconv::Variant::ZhHant)
}
