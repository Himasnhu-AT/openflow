//! OWNER: team B (data). JSON text identical to Foundation's `JSONEncoder`, so files written by either app diff
//! cleanly.
//!
//! Foundation specifics reproduced here (verified against `swiftc` output, see `testdata/swift/`):
//! - `.prettyPrinted`: 2-space indent, `"key" : value`, empty containers print as `[\n\n<indent>]`, no final newline.
//! - `.sortedKeys`: keys in code-point order (same as byte order of UTF-8).
//! - `/` is escaped as `\/`; `\b \f \n \r \t` short escapes; other controls as `\u00xx` (lowercase hex);
//!   everything else (DEL, U+2028, emoji) is written raw.
//! - Doubles: integral values have no `.0`; exponent form (`1e-05`, `1e+16`) when the decimal exponent is < -4
//!   or >= 16, as Swift's `description` does.
//!
//! Compact output (plain `JSONEncoder()`, used for history.json and stats.json) is the same without whitespace;
//! keys are sorted there too (Swift's order is unspecified).

use serde::Serialize;
use serde_json::Value;

/// `JSONEncoder` with `[.prettyPrinted, .sortedKeys]`.
pub fn to_pretty<T: Serialize + ?Sized>(value: &T) -> anyhow::Result<String> {
    let v = serde_json::to_value(value)?;
    let mut out = String::new();
    write_value(&mut out, &v, Some(0));
    Ok(out)
}

/// Plain `JSONEncoder()` (no whitespace).
pub fn to_compact<T: Serialize + ?Sized>(value: &T) -> anyhow::Result<String> {
    let v = serde_json::to_value(value)?;
    let mut out = String::new();
    write_value(&mut out, &v, None);
    Ok(out)
}

/// Drops `null` members of a top-level object: Swift's `decodeIfPresent` treats `null` like a missing key, which
/// serde's `#[serde(default)]` does not.
pub fn strip_top_level_nulls(v: &mut Value) {
    if let Value::Object(m) = v {
        m.retain(|_, x| !x.is_null());
    }
}

fn indent(out: &mut String, level: usize) {
    for _ in 0..level {
        out.push_str("  ");
    }
}

fn write_value(out: &mut String, v: &Value, level: Option<usize>) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                out.push_str(&i.to_string())
            } else if let Some(u) = n.as_u64() {
                out.push_str(&u.to_string())
            } else {
                out.push_str(&format_double(n.as_f64().unwrap_or(0.0)))
            }
        }
        Value::String(s) => write_string(out, s),
        Value::Array(items) => {
            let items: Vec<&Value> = items.iter().collect();
            write_container(out, '[', ']', &items, level, write_value);
        }
        Value::Object(map) => {
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            write_container(out, '{', '}', &entries, level, |out, (k, v), lvl| {
                write_string(out, k);
                out.push_str(if lvl.is_some() { " : " } else { ":" });
                write_value(out, v, lvl);
            });
        }
    }
}

fn write_container<T: Copy>(
    out: &mut String,
    open: char,
    close: char,
    items: &[T],
    level: Option<usize>,
    mut write: impl FnMut(&mut String, T, Option<usize>),
) {
    out.push(open);
    match level {
        None => {
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(out, *item, None);
            }
        }
        Some(l) => {
            out.push('\n');
            if items.is_empty() {
                out.push('\n');
            }
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(",\n");
                }
                indent(out, l + 1);
                write(out, *item, Some(l + 1));
            }
            if !items.is_empty() {
                out.push('\n');
            }
            indent(out, l);
        }
    }
    out.push(close);
}

fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '/' => out.push_str("\\/"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Swift `Double.description` without a trailing `.0`.
pub fn format_double(f: f64) -> String {
    if !f.is_finite() {
        // Foundation refuses to encode these; never produced by the app.
        return "null".into();
    }
    if f == 0.0 {
        return if f.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    let sci = format!("{f:e}"); // shortest digits, e.g. "1.2345678901234568e16", "5e-324"
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    if (-4..16).contains(&exp) {
        format!("{f}") // Rust's Display never uses an exponent and drops ".0"
    } else {
        format!("{mantissa}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_match_foundation() {
        let nums: Vec<f64> = vec![
            0.0,
            1.0,
            0.5,
            1e-7,
            1e21,
            123456789.123,
            780000000.123456,
            -3.25,
            1e16,
            12345678901234567.0,
        ];
        assert_eq!(to_pretty(&nums).unwrap(), include_str!("../testdata/swift/nums.json"));
        let more = [0.0001, 0.00001, 9007199254740992.0, 1e15, 100.0, -0.0, 5e-324, 1.5e300];
        let got: Vec<String> = more.iter().map(|f| format_double(*f)).collect();
        assert_eq!(
            got,
            ["0.0001", "1e-05", "9007199254740992", "1000000000000000", "100", "-0", "5e-324", "1.5e+300"]
        );
    }

    #[test]
    fn strings_match_foundation() {
        let v = vec!["a\u{08}b\u{0c}c\r\u{1f}\u{1b}", "\u{7f}", "a/b \"q\" \\ é 😀"];
        assert_eq!(
            to_compact(&v).unwrap(),
            "[\"a\\bb\\fc\\r\\u001f\\u001b\",\"\u{7f}\",\"a\\/b \\\"q\\\" \\\\ é 😀\"]"
        );
    }

    #[test]
    fn sorted_keys_and_empty_containers() {
        let v = serde_json::json!({"z": 1, "B": 2, "a": 3, "é": 4, "_x": 5, "Z": 6, "e\u{301}x": 7, "f": {}, "g": []});
        assert_eq!(
            to_pretty(&v).unwrap(),
            "{\n  \"B\" : 2,\n  \"Z\" : 6,\n  \"_x\" : 5,\n  \"a\" : 3,\n  \"e\u{301}x\" : 7,\n  \"f\" : {\n\n  },\n  \
             \"g\" : [\n\n  ],\n  \"z\" : 1,\n  \"é\" : 4\n}"
        );
        assert_eq!(to_compact(&serde_json::json!({"b": [], "a": {}})).unwrap(), "{\"a\":{},\"b\":[]}");
    }
}
