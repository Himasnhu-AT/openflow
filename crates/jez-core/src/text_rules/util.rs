//! Helpers that reproduce the Foundation / NSRegularExpression behaviour TextRules.swift relies on.

use fancy_regex::{Captures, Regex};
use unicode_properties::{GeneralCategory, GeneralCategoryGroup, UnicodeGeneralCategory};

/// Compiles a pattern (all patterns are constants, so a bad one is a programming error).
pub(crate) fn rx(p: &str) -> Regex {
    Regex::new(p).unwrap_or_else(|e| panic!("bad regex {p:?}: {e}"))
}

/// `CharacterSet.punctuationCharacters`: Unicode general category P*.
pub(crate) fn is_punct(c: char) -> bool {
    c.general_category_group() == GeneralCategoryGroup::Punctuation
}

/// `CharacterSet.whitespaces`: general category Zs plus tab (no newlines).
pub(crate) fn is_hspace(c: char) -> bool {
    c == '\t' || c.general_category() == GeneralCategory::SpaceSeparator
}

/// `CharacterSet.alphanumerics`: general categories L*, M* and N*.
pub(crate) fn is_alnum(c: char) -> bool {
    matches!(
        c.general_category_group(),
        GeneralCategoryGroup::Letter | GeneralCategoryGroup::Mark | GeneralCategoryGroup::Number
    )
}

/// `trimmingCharacters(in: .punctuationCharacters)`.
pub(crate) fn trim_punct(s: &str) -> &str {
    s.trim_matches(is_punct)
}

/// `trimmingCharacters(in: CharacterSet.punctuationCharacters.subtracting(keep))`.
pub(crate) fn trim_punct_except<'a>(s: &'a str, keep: &[char]) -> &'a str {
    s.trim_matches(|c: char| is_punct(c) && !keep.contains(&c))
}

/// `trimmingCharacters(in: .whitespaces)`.
pub(crate) fn trim_hspace(s: &str) -> &str {
    s.trim_matches(is_hspace)
}

/// `split(separator: " ")` (empty pieces omitted).
pub(crate) fn split_spaces(s: &str) -> Vec<&str> {
    s.split(' ').filter(|w| !w.is_empty()).collect()
}

/// First character uppercased + the rest (`prefix(1).uppercased() + dropFirst()`).
pub(crate) fn upper_first(s: &str) -> String {
    let mut it = s.chars();
    match it.next() {
        Some(c) => c.to_uppercase().collect::<String>() + it.as_str(),
        None => String::new(),
    }
}

/// First match, `None` on no match (or on a backtrack-limit error, which the patterns here never reach).
pub(crate) fn find_caps<'t>(re: &Regex, text: &'t str) -> Option<Captures<'t>> {
    re.captures(text).ok().flatten()
}

pub(crate) fn is_match(re: &Regex, text: &str) -> bool {
    re.is_match(text).unwrap_or(false)
}

/// `stringByReplacingMatches(withTemplate:)` with an NSRegularExpression template: `$n` is group n (unmatched or
/// missing groups give ""), `\x` is a literal x, anything else is literal.
pub(crate) fn replace_all(re: &Regex, text: &str, template: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for caps in re.captures_iter(text) {
        let Ok(caps) = caps else { break };
        let m = caps.get(0).expect("group 0");
        out.push_str(&text[last..m.start()]);
        expand(template, &caps, &mut out);
        last = m.end();
    }
    out.push_str(&text[last..]);
    out
}

fn expand(template: &str, caps: &Captures, out: &mut String) {
    let chars: Vec<char> = template.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            out.push(chars[i + 1]);
            i += 2;
        } else if c == '$' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
            // ICU takes as many digits as still name an existing group (at least one).
            let mut n = chars[i + 1].to_digit(10).unwrap() as usize;
            i += 2;
            while i < chars.len() && chars[i].is_ascii_digit() {
                let next = n * 10 + chars[i].to_digit(10).unwrap() as usize;
                if next >= caps.len() {
                    break;
                }
                n = next;
                i += 1;
            }
            if let Some(g) = caps.get(n) {
                out.push_str(g.as_str());
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
}

/// C `printf("%g")`: 6 significant digits, trailing zeros removed, exponent form outside 1e-4 ..< 1e6.
pub(crate) fn format_g(x: f64) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if x == 0.0 {
        return if x.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    let sci = format!("{:.5e}", x); // e.g. "1.50000e6"
    let (mant, exp) = sci.split_once('e').expect("exponent");
    let exp: i32 = exp.parse().expect("exponent digits");
    if !(-4..6).contains(&exp) {
        let mant = strip_zeros(mant);
        let sign = if exp < 0 { '-' } else { '+' };
        format!("{mant}e{sign}{:02}", exp.abs())
    } else {
        let prec = (5 - exp) as usize;
        strip_zeros(&format!("{:.*}", prec, x)).to_string()
    }
}

fn strip_zeros(s: &str) -> &str {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.')
    } else {
        s
    }
}

/// Swift `Double(String)` (strtod on the whole string): Rust's parser plus hex floats are rare enough to skip.
pub(crate) fn parse_double(s: &str) -> Option<f64> {
    if s.is_empty() {
        return None;
    }
    s.parse::<f64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printf_g() {
        assert_eq!(format_g(1.5), "1.5");
        assert_eq!(format_g(2.0), "2");
        assert_eq!(format_g(1_500_000.0), "1.5e+06");
        assert_eq!(format_g(0.0001), "0.0001");
        assert_eq!(format_g(0.00001), "1e-05");
        assert_eq!(format_g(123456.0), "123456");
        assert_eq!(format_g(0.1 + 0.2), "0.3");
    }

    #[test]
    fn categories() {
        assert!(is_punct('\''));
        assert!(is_punct('—'));
        assert!(!is_punct('$'));
        assert!(is_hspace('\u{a0}'));
        assert!(!is_hspace('\n'));
    }
}
