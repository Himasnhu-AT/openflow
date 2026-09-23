//! Spoken commands, emoji and numbers (applied in every tone, including verbatim).

use fancy_regex::Regex;
use once_cell::sync::Lazy;

use super::util::{find_caps, format_g, parse_double, replace_all, rx, trim_punct};

static COMMANDS: Lazy<Vec<(Regex, &'static str)>> = Lazy::new(|| {
    vec![
        (rx(r"[,.]?\s*\b(new paragraph|paragraph break)\b[,.]?\s*"), "\n\n"),
        (rx(r"[,.]?\s*\b(new line|newline|line break)\b[,.]?\s*"), "\n"),
        (rx(r"[,.]?\s*\b(bullet point|dot point|new bullet|next bullet)\b[,.]?\s*"), "\n- "),
        (rx(r"\s*\b(full stop|fullstop)\b[,.]?"), "."),
        (rx(r"\s*\bquestion mark\b[,.]?"), "?"),
        (rx(r"\s*\bexclamation (mark|point)\b[,.]?"), "!"),
        // "sam at example dot com" -> sam@example.com ; "acme hub dot com dot au" -> acmehub.com.au
        (rx(r"\b([\w.-]+) at ([\w-]+)((?: dot [a-z]{2,4})+)\b"), "$1@$2$3"),
        (rx(r"\b([\w-]+)((?: dot [a-z]{2,4})+)\b"), "$1$2"),
    ]
});
static DOT: Lazy<Regex> = Lazy::new(|| rx(r" dot ([a-z]{2,4})\b"));

/// Spoken emoji names, longest first so "crying laugh" beats "crying".
pub static EMOJI_NAMES: Lazy<Vec<(&'static str, &'static str)>> = Lazy::new(|| {
    let mut v: Vec<(&str, &str)> = vec![
        ("laugh cry", "😂"), ("laughing cry", "😂"), ("laugh crying", "😂"), ("laughing crying", "😂"), ("cry laugh", "😂"),
        ("crying laugh", "😂"), ("crying laughing", "😂"), ("cry laughing", "😂"), ("tears of joy", "😂"), ("laughing", "😂"), ("laugh", "😂"),
        ("rolling on the floor", "🤣"), ("rofl", "🤣"), ("happy cry", "🥲"), ("smiling cry", "🥲"),
        ("sobbing", "😭"), ("loud crying", "😭"), ("bawling", "😭"), ("crying", "😢"), ("cry", "😢"), ("sad", "😢"),
        ("heart eyes", "😍"), ("blowing kiss", "😘"), ("kiss", "😘"), ("sweat smile", "😅"), ("nervous laugh", "😅"),
        ("grinning", "😁"), ("grin", "😁"), ("smiley", "😊"), ("smiling", "😊"), ("smile", "😊"), ("slight smile", "🙂"),
        ("winking", "😉"), ("wink", "😉"), ("sunglasses", "😎"), ("cool", "😎"), ("thinking", "🤔"), ("grimace", "😬"), ("grimacing", "😬"),
        ("eye roll", "🙄"), ("rolling eyes", "🙄"), ("shrug", "🤷"), ("shrugging", "🤷"), ("skull", "💀"), ("neutral", "😐"),
        ("sleeping", "😴"), ("sleepy", "😴"), ("vomit", "🤮"), ("puke", "🤮"), ("partying", "🥳"), ("party face", "🥳"),
        ("thumbs up", "👍"), ("thumbs down", "👎"), ("clapping", "👏"), ("clap", "👏"), ("praying", "🙏"), ("pray", "🙏"),
        ("folded hands", "🙏"), ("thank you hands", "🙏"), ("fire", "🔥"), ("red heart", "❤️"), ("heart", "❤️"), ("broken heart", "💔"),
        ("hundred", "💯"), ("100", "💯"), ("party", "🎉"), ("tada", "🎉"), ("celebration", "🎉"), ("check", "✅"), ("tick", "✅"),
        ("cross", "❌"), ("rocket", "🚀"), ("eyes", "👀"), ("wave", "👋"), ("waving", "👋"), ("muscle", "💪"), ("flex", "💪"),
        ("ok hand", "👌"), ("peace", "✌️"), ("star", "⭐"), ("sparkles", "✨"), ("poop", "💩"), ("money", "💰"), ("beer", "🍺"),
        ("coffee", "☕"), ("pizza", "🍕"), ("cake", "🎂"), ("sun", "☀️"), ("rain", "🌧️"), ("dog", "🐶"), ("cat", "🐱"),
    ];
    v.sort_by_key(|e| std::cmp::Reverse(e.0.chars().count()));
    v
});

static EMOJI: Lazy<Regex> = Lazy::new(|| {
    // NSRegularExpression.escapedPattern leaves spaces alone, so the " " -> [\s,\-]+ substitution applies.
    let names = EMOJI_NAMES
        .iter()
        .map(|(n, _)| regex::escape(n).replace(' ', r"[\s,\-]+"))
        .collect::<Vec<_>>()
        .join("|");
    rx(&format!(r"(?i)\s*[,.]?\s*\b(?:(?:a|an|the)\s+)?({names})[\s,\-]+emojis?\b[,.]?"))
});
static EMOJI_SEP: Lazy<Regex> = Lazy::new(|| rx(r"[\s,\-]+"));

pub fn apply_emoji(text: &str) -> String {
    let mut t = text.to_string();
    while let Some(c) = find_caps(&EMOJI, &t) {
        let whole = c.get(0).expect("match");
        let name = c.get(1).expect("name");
        let key = replace_all(&EMOJI_SEP, &name.as_str().to_lowercase(), " ");
        let glyph = EMOJI_NAMES.iter().find(|(n, _)| *n == key).map(|e| e.1).unwrap_or("");
        let before = &t[..whole.start()];
        let space = if before.is_empty() || before.chars().last().is_some_and(char::is_whitespace) { "" } else { " " };
        let (start, end) = (whole.start(), whole.end());
        t.replace_range(start..end, &format!("{space}{glyph}"));
    }
    t
}

// MARK: Spoken numbers ("two billion dollars" -> "$2 billion", "one point five billion" -> "1.5 billion")

fn small_number(w: &str) -> Option<i64> {
    Some(match w {
        "zero" => 0, "one" => 1, "two" => 2, "three" => 3, "four" => 4, "five" => 5, "six" => 6, "seven" => 7,
        "eight" => 8, "nine" => 9, "ten" => 10, "eleven" => 11, "twelve" => 12, "thirteen" => 13, "fourteen" => 14,
        "fifteen" => 15, "sixteen" => 16, "seventeen" => 17, "eighteen" => 18, "nineteen" => 19, "twenty" => 20,
        "thirty" => 30, "forty" => 40, "fifty" => 50, "sixty" => 60, "seventy" => 70, "eighty" => 80, "ninety" => 90,
        _ => return None,
    })
}

fn scale(w: &str) -> Option<f64> {
    Some(match w {
        "hundred" => 100.0,
        "thousand" => 1_000.0,
        "million" => 1_000_000.0,
        "billion" => 1_000_000_000.0,
        "trillion" => 1_000_000_000_000.0,
        _ => return None,
    })
}

fn is_big_scale(w: &str) -> bool {
    matches!(w, "million" | "billion" | "trillion")
}

/// A parsed spoken number: value, whether a decimal point was spoken, the last big scale word, tokens consumed.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedNumber {
    pub value: f64,
    pub has_decimal: bool,
    pub last_scale: Option<String>,
    pub consumed: usize,
}

pub fn apply_numbers(text: &str) -> String {
    // Work on space-separated tokens, keeping the original separators so line breaks survive.
    let mut tokens: Vec<String> = text.split(' ').map(String::from).collect();
    let mut i = 0;
    while i < tokens.len() {
        let Some(p) = parse_number(&tokens, i) else {
            i += 1;
            continue;
        };
        let mut end = i + p.consumed;
        let trailing = trailing_punctuation(&tokens[i + p.consumed - 1]);
        let mut unit = "";
        let next = if end < tokens.len() { trim_punct(&tokens[end].to_lowercase()).to_string() } else { String::new() };
        let next_trail = if end < tokens.len() { trailing_punctuation(&tokens[end]) } else { String::new() };
        if next == "dollars" || next == "dollar" || next == "bucks" {
            unit = "$";
            end += 1;
        } else if next == "percent"
            || (next == "per" && end + 1 < tokens.len() && tokens[end + 1].to_lowercase().starts_with("cent"))
        {
            unit = "%";
            end += if next == "per" { 2 } else { 1 };
        } else if next == "k" {
            unit = "k";
            end += 1;
        }
        let spoken_digits = tokens[i].chars().next().is_some_and(char::is_numeric);
        let worth = p.value >= 10.0 || p.has_decimal || !unit.is_empty() || p.last_scale.is_some() || spoken_digits;
        if !worth || (spoken_digits && unit.is_empty() && p.last_scale.is_none()) {
            i += 1;
            continue;
        }
        let body = match &p.last_scale {
            Some(s) if is_big_scale(s) => {
                let n = p.value / scale(s).expect("scale");
                let num = if n == n.round() { (n as i64).to_string() } else { format_g(n) };
                format!("{num} {s}")
            }
            _ => {
                if p.has_decimal {
                    format_g(p.value)
                } else {
                    format_int(p.value as i64)
                }
            }
        };
        let mut replacement = if unit == "$" {
            format!("${body}")
        } else {
            body + if unit == "%" || unit == "k" { unit } else { "" }
        };
        replacement += if unit.is_empty() { &trailing } else { &next_trail };
        tokens.splice(i..end, [replacement]);
        i += 1;
    }
    tokens.join(" ")
}

/// Parses number words starting at `from`. Digits like "1.5" or "20" count too.
pub fn parse_number(tokens: &[String], from: usize) -> Option<ParsedNumber> {
    let word = |k: usize| trim_punct(&tokens[k].to_lowercase()).to_string();
    let mut i = from;
    let mut total = 0.0;
    let mut current = 0.0;
    let mut has_decimal = false;
    let mut last_scale: Option<String> = None;
    let mut saw_word = false;
    let mut decimal_scale = 0.1;
    let mut after_point = false;
    let w0 = word(i);
    if let Some(d) = parse_double(&w0.replace(',', "")) {
        if !w0.is_empty() {
            total = d;
            has_decimal = w0.contains('.');
            i += 1;
            if i < tokens.len() {
                let w = word(i);
                if let Some(sc) = scale(&w) {
                    if is_big_scale(&w) {
                        total *= sc;
                        last_scale = Some(w);
                        i += 1;
                    }
                }
            }
            return Some(ParsedNumber { value: total, has_decimal, last_scale, consumed: i - from });
        }
    }
    while i < tokens.len() {
        let w = word(i);
        if after_point {
            if let Some(n) = small_number(&w).filter(|n| *n < 10) {
                total += n as f64 * decimal_scale;
                decimal_scale /= 10.0;
                i += 1;
                continue;
            }
            if let Some(sc) = scale(&w) {
                if is_big_scale(&w) {
                    total *= sc;
                    last_scale = Some(w);
                    i += 1;
                }
            }
            break;
        }
        if (w == "a" || w == "an") && i + 1 < tokens.len() && scale(&word(i + 1)).is_some() {
            current = 1.0;
            i += 1;
            saw_word = true;
            continue;
        }
        if let Some(n) = small_number(&w) {
            current += n as f64;
            i += 1;
            saw_word = true;
            continue;
        }
        if let Some(sc) = scale(&w) {
            if !saw_word {
                return None;
            }
            if w == "hundred" {
                current = (if current == 0.0 { 1.0 } else { current }) * sc;
            } else {
                total += (if current == 0.0 { 1.0 } else { current }) * sc;
                current = 0.0;
                last_scale = if is_big_scale(&w) { Some(w) } else { None };
            }
            i += 1;
            continue;
        }
        if w == "and" && saw_word && i + 1 < tokens.len() {
            let n = word(i + 1);
            if small_number(&n).is_some() || scale(&n).is_some() {
                i += 1;
                continue;
            }
        }
        if w == "point" && saw_word && i + 1 < tokens.len() && small_number(&word(i + 1)).is_some_and(|n| n < 10) {
            after_point = true;
            has_decimal = true;
            total += current;
            current = 0.0;
            i += 1;
            continue;
        }
        break;
    }
    if !saw_word {
        return None;
    }
    total += current;
    Some(ParsedNumber { value: total, has_decimal, last_scale, consumed: i - from })
}

fn trailing_punctuation(s: &str) -> String {
    let n = s.chars().rev().take_while(|c| matches!(c, ',' | '.' | '?' | '!')).count();
    let chars: Vec<char> = s.chars().collect();
    chars[chars.len() - n..].iter().collect()
}

/// NumberFormatter .decimal, grouping only from 10,000 up.
fn format_int(n: i64) -> String {
    let s = n.unsigned_abs().to_string();
    if n < 10_000 {
        return n.to_string();
    }
    let mut out = String::new();
    for (k, c) in s.chars().enumerate() {
        if k > 0 && (s.len() - k).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub fn apply_commands(text: &str) -> String {
    let mut t = apply_numbers(&apply_emoji(text));
    for (r, rep) in COMMANDS.iter() {
        t = replace_all(r, &t, rep);
    }
    // Second pass turns the " dot xx" runs left by the email/url templates into ".xx".
    t = replace_all(&DOT, &t, ".$1");
    t.replace(" \n", "\n").trim().to_string()
}
