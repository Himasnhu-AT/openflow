//! UTF-16 and grapheme helpers shared by the field reader and the live typer.
//! Accessibility APIs (macOS AX, Windows UIA) count text in UTF-16 units; backspace deletes one user-perceived
//! character (grapheme cluster), which is what Swift's `String.count` counts.

use jez_core::FieldContext;
use unicode_segmentation::UnicodeSegmentation;

/// Number of user-perceived characters (Swift `String.count`).
pub fn char_count(s: &str) -> usize {
    s.graphemes(true).count()
}

/// Number of UTF-16 code units (Swift `String.utf16.count`).
pub fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// The first `n` characters of `s` (Swift `String(s.prefix(n))`).
pub fn prefix_chars(s: &str, n: usize) -> &str {
    match s.grapheme_indices(true).nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// `s` without its first `n` characters (Swift `s.dropFirst(n)`).
pub fn drop_chars(s: &str, n: usize) -> &str {
    &s[prefix_chars(s, n).len()..]
}

/// Length in characters of the common prefix of `a` and `b` (Swift `zip(a, b).prefix { $0 == $1 }.count`).
pub fn common_prefix_chars(a: &str, b: &str) -> usize {
    a.graphemes(true).zip(b.graphemes(true)).take_while(|(x, y)| x == y).count()
}

/// Builds a field context from a field's value and the reported selection start (FieldContext.capture steps 4–6).
/// `selection_location` is None when the field did not report a selected-text range.
pub fn context_from_value(window_title: String, value: &str, selection_location: Option<isize>) -> FieldContext {
    let units: Vec<u16> = value.encode_utf16().collect();
    let mut ctx = FieldContext { window_title, ..Default::default() };
    let mut cursor = units.len();
    if let Some(loc) = selection_location {
        cursor = (loc.max(0) as usize).min(units.len());
        ctx.cursor = Some(cursor);
    }
    let before = &units[..cursor];
    ctx.before = String::from_utf16_lossy(&before[before.len().saturating_sub(3000)..]);
    let after = &units[cursor..];
    ctx.after = String::from_utf16_lossy(&after[..after.len().min(600)]);
    ctx
}

/// First `n` characters of a window title (Swift `String(title.prefix(120))`).
pub fn truncate_title(title: &str) -> String {
    prefix_chars(title, 120).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graphemes() {
        assert_eq!(char_count("héllo👍🏽"), 6);
        assert_eq!(utf16_len("👍🏽"), 4);
        assert_eq!(prefix_chars("ab👍🏽c", 3), "ab👍🏽");
        assert_eq!(drop_chars("ab👍🏽c", 3), "c");
        assert_eq!(drop_chars("ab", 5), "");
        assert_eq!(common_prefix_chars("hello world", "hello there"), 6);
    }

    #[test]
    fn context_windows() {
        let ctx = context_from_value("T".into(), "hello world", Some(5));
        assert_eq!(ctx.before, "hello");
        assert_eq!(ctx.after, " world");
        assert_eq!(ctx.cursor, Some(5));
        let ctx = context_from_value(String::new(), "abc", None);
        assert_eq!((ctx.before.as_str(), ctx.after.as_str(), ctx.cursor), ("abc", "", None));
        let ctx = context_from_value(String::new(), "abc", Some(99));
        assert_eq!(ctx.cursor, Some(3));
        let ctx = context_from_value(String::new(), "abc", Some(-4));
        assert_eq!((ctx.cursor, ctx.after.as_str()), (Some(0), "abc"));
        let long = "x".repeat(5000);
        let ctx = context_from_value(String::new(), &long, Some(4000));
        assert_eq!((ctx.before.len(), ctx.after.len()), (3000, 600));
    }
}
