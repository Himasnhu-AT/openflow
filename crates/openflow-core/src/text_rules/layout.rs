//! Casual fast path and the layout touch-ups that run after the model.

use fancy_regex::Regex;
use once_cell::sync::Lazy;

use super::util::{find_caps, is_match, replace_all, rx, split_spaces, trim_hspace, trim_punct};

// MARK: Casual fast path

static CAPITAL_I: Lazy<Regex> = Lazy::new(|| rx(r"\bI\b"));
static SHOUTY: Lazy<Regex> = Lazy::new(|| rx(r"[!;:]+"));
static LAUGH: Lazy<Regex> = Lazy::new(|| rx(r"\b(haha+|lol|lmao)[.,]"));
static SPACES: Lazy<Regex> = Lazy::new(|| rx(r"\s{2,}"));

/// casual: lowercase sentence starts and "i", the transcriber's commas and full stops kept (including the last).
/// super casual: no commas, full stops only between sentences, none at the end. Dictionary terms keep their spelling.
pub fn casual(text: &str, keeping: &[String], super_casual: bool) -> String {
    let mut t = lowercase_sentence_starts(text);
    t = replace_all(&CAPITAL_I, &t, "i");
    t = replace_all(&SHOUTY, &t, "");
    t = replace_all(&LAUGH, &t, "$1");
    if super_casual {
        t = t.replace(',', "");
    }
    t = trim_hspace(&replace_all(&SPACES, &t, " ")).to_string();
    if super_casual {
        while t.ends_with('.') || t.ends_with(',') {
            t.pop();
        }
    }
    for term in keeping.iter().filter(|t| t.chars().count() >= 3) {
        t = t.replace(&term.to_lowercase(), term);
    }
    t
}

fn lowercase_sentence_starts(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at_start = true;
    for ch in text.chars() {
        if at_start && ch.is_alphabetic() {
            out.extend(ch.to_lowercase());
            at_start = false;
            continue;
        }
        if ch == '.' || ch == '?' || ch == '!' {
            at_start = true;
        } else if !ch.is_whitespace() {
            at_start = false;
        }
        out.push(ch);
    }
    out
}

// MARK: Layout touch-up (after the model)

static MESSAGE_GREETING: Lazy<Regex> = Lazy::new(|| {
    rx(r"(?i)^((?:dear|hi|hey|hello|good morning|good afternoon|good evening|morning|afternoon|evening)\b(?:[^,.!\n]{0,40}[,.!]|\s+[A-Za-z][\w'-]*(?=\s)))\s*")
});
static MESSAGE_SIGNOFF: Lazy<Regex> = Lazy::new(|| {
    rx(r"(?i)\s+((?:kind regards|best regards|warm regards|warmest regards|regards|cheers|many thanks|thanks|thank you|best|all the best|talk soon|take care|speak soon)[,.]?)(?:\s+([A-Za-z][\w'-]*(?:\s+[A-Za-z][\w'-]*)?))?[.]?\s*$")
});

/// A dictation that opens with a greeting and closes with a sign-off is laid out as a message.
pub fn layout_message(text: &str) -> String {
    if text.contains('\n') {
        return text.to_string();
    }
    let Some(g) = find_caps(&MESSAGE_GREETING, text) else { return text.to_string() };
    let Some(so) = find_caps(&MESSAGE_SIGNOFF, text) else { return text.to_string() };
    let greeting = g.get(1).expect("greeting").as_str();
    let after_greeting = g.get(0).expect("match").end();
    let signoff_range = so.get(0).expect("match");
    let signoff = so.get(1).expect("signoff").as_str();
    if signoff_range.start() <= after_greeting {
        return text.to_string();
    }
    let mut body = trim_hspace(&text[after_greeting..signoff_range.start()]).to_string();
    if body.ends_with(',') {
        body.pop();
    }
    if split_spaces(&body).len() < 2 {
        return text.to_string();
    }
    let mut out = format!("{greeting}\n\n{body}\n\n{signoff}");
    if let Some(name) = so.get(2) {
        out.push('\n');
        out.push_str(name.as_str());
    }
    out
}

/// The model sometimes repeats the text that was already before the cursor. Drop such an echo.
pub fn strip_echo(output: &str, before: &str) -> String {
    let before_words = split_spaces(before);
    let tail = &before_words[before_words.len().saturating_sub(12)..];
    if tail.len() < 4 {
        return output.to_string();
    }
    let out_words = split_spaces(output);
    let norm = |w: &str| trim_punct(&w.to_lowercase()).to_string();
    let t: Vec<String> = tail.iter().map(|w| norm(w)).collect();
    let o: Vec<String> = out_words.iter().map(|w| norm(w)).collect();
    let mut n = t.len().min(o.len());
    while n >= 4 {
        for j in 0..=2.min(o.len() - n) {
            if t[t.len() - n..] == o[j..j + n] {
                return out_words[j + n..].join(" ");
            }
        }
        n -= 1;
    }
    output.to_string()
}

static GREETING: Lazy<Regex> = Lazy::new(|| {
    rx(r"(?i)^((?:hi|hey|hello|dear|good morning|good afternoon|good evening|morning|afternoon|evening)\b(?:[^,.!\n]{0,40}[,.!]|\s+[A-Za-z][\w'-]*(?=\s)))\s+(?=\S)")
});

/// If the model laid the text out as a message but left the greeting glued to the first sentence, put the
/// greeting on its own line.
pub fn layout_greeting(text: &str) -> String {
    if !text.contains("\n\n") {
        return text.to_string();
    }
    let Some(m) = find_caps(&GREETING, text) else { return text.to_string() };
    let head = m.get(1).expect("head").as_str();
    let rest = &text[m.get(0).expect("match").end()..];
    let capitalised = head.chars().next().is_some_and(char::is_uppercase); // casual tone stays lowercase
    let mut it = rest.chars();
    let first: String = match it.next() {
        Some(c) if capitalised => c.to_uppercase().collect(),
        Some(c) => c.to_string(),
        None => String::new(),
    };
    format!("{head}\n\n{first}{}", it.as_str())
}

/// Spoken enumerations become a bullet list when there are at least two markers. Runs after the model.
static LIST_MARKER: Lazy<Regex> = Lazy::new(|| {
    rx(r"(?i)(^|[.,;:!?]\s*|\s)(?:(?:firstly|secondly|thirdly|fourthly|fifthly|first of all|second of all|number (?:one|two|three|four|five|six))|(?<=[.,;:!?]\s)(?:first|second|third|fourth|fifth)|^(?:first|second|third))\b[,:]?\s*")
});
static LIST_COMMA: Lazy<Regex> = Lazy::new(|| rx(r"[,;]\n- "));
static LIST_LEAD: Lazy<Regex> = Lazy::new(|| rx(r"^\n- "));

pub fn apply_lists(text: &str) -> String {
    let matches: Vec<_> = LIST_MARKER.captures_iter(text).filter_map(Result::ok).collect();
    if matches.len() < 2 {
        return text.to_string();
    }
    let mut t = text.to_string();
    for m in matches.iter().rev() {
        let whole = m.get(0).expect("match");
        let lead = m.get(1).map_or("", |g| g.as_str());
        let keep = trim_hspace(lead); // the punctuation that ended the previous clause
        t.replace_range(whole.start()..whole.end(), &format!("{keep}\n- "));
    }
    let t = replace_all(&LIST_COMMA, &t, ".\n- ");
    let t = replace_all(&LIST_LEAD, &t, "- ");
    t.trim().to_string()
}

/// Adds a missing "?" to sentences that are unmistakably questions by their opening words.
static QUESTION_START: Lazy<Regex> = Lazy::new(|| {
    rx(r"(?i)^(?:(?:are|can|could|do|does|did|is|will|would|should|shall|have|has|were|was|am)\s+(?:you|we|i|it|there|they|he|she|this|that|anyone|someone)\b|(?:what|when|where|who|why|how)['’](?:s|re|d|ll)\s+\w+|(?:what|when|where|who|why|how)\s+(?:is|are|do|does|did|can|could|will|would|should|have|has|was|were|about|time|many|much|long)\b)")
});
static BULLET: Lazy<Regex> = Lazy::new(|| rx(r"^(?:- |\d+[.)] )"));

pub fn question_marks(text: &str) -> String {
    text.split('\n')
        .map(|line| {
            let mut out = String::new();
            let mut sentence = String::new();
            let flush = |sentence: &mut String, out: &mut String, terminator: Option<char>| {
                let body = replace_all(&BULLET, trim_hspace(sentence), ""); // list bullets
                let is_question = is_match(&QUESTION_START, &body);
                out.push_str(sentence);
                if is_question && (terminator.is_none() || terminator == Some('.')) {
                    out.push('?');
                } else if let Some(t) = terminator {
                    out.push(t);
                }
                sentence.clear();
            };
            for ch in line.chars() {
                if ch == '.' || ch == '?' || ch == '!' {
                    flush(&mut sentence, &mut out, Some(ch));
                } else {
                    sentence.push(ch);
                }
            }
            if !trim_hspace(&sentence).is_empty() {
                flush(&mut sentence, &mut out, None);
            }
            out
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// MARK: Counted lists ("three things: a, b, c" -> numbered list)

static COUNTED_LEAD: Lazy<Regex> = Lazy::new(|| {
    rx(r"(?i)\b(two|three|four|five|six|2|3|4|5|6)\s+(things|reasons|points|steps|options|ways|issues|updates|questions|items|ideas|problems|tips|takeaways|decisions|requests|asks)\b([^:.\n]{0,80})[:.]\s*")
});
static SENTENCE_END: Lazy<Regex> = Lazy::new(|| rx(r"[.!?](?=\s|$)|\n"));
static SEMICOLON: Lazy<Regex> = Lazy::new(|| rx(r"\s*;\s*"));
static COMMA_AND: Lazy<Regex> = Lazy::new(|| rx(r",\s*(and|or)\s+"));
static AND_OR: Lazy<Regex> = Lazy::new(|| rx(r"\s+(and|or)\s+"));

fn count_word(w: &str) -> Option<usize> {
    Some(match w {
        "two" | "2" => 2,
        "three" | "3" => 3,
        "four" | "4" => 4,
        "five" | "5" => 5,
        "six" | "6" => 6,
        _ => return None,
    })
}

/// "there are three things it does well: it's fast, it's accurate, it gets names right." becomes the lead-in
/// followed by a numbered list, when the items after the colon really split into that many.
pub fn apply_counted_lists(text: &str) -> String {
    if text.contains("\n1.") || text.contains("\n- ") {
        return text.to_string();
    }
    let Some(m) = find_caps(&COUNTED_LEAD, text) else { return text.to_string() };
    let whole_end = m.get(0).expect("match").end();
    let Some(n) = count_word(&m.get(1).expect("count").as_str().to_lowercase()) else { return text.to_string() };
    // Items run to the end of the sentence that follows the colon.
    let rest = &text[whole_end..];
    let (body, mut tail) = match SENTENCE_END.find(rest).ok().flatten() {
        Some(end) => (&rest[..end.start()], trim_hspace(&rest[end.end()..]).to_string()),
        None => (rest, String::new()),
    };
    let joined = replace_all(&COMMA_AND, &replace_all(&SEMICOLON, body, ","), ",");
    let mut items: Vec<String> =
        joined.split(',').filter(|s| !s.is_empty()).map(|s| trim_hspace(s).to_string()).filter(|s| !s.is_empty()).collect();
    if items.len() + 1 == n {
        let last = items.last().cloned().unwrap_or_default();
        if let Some(r) = AND_OR.find(&last).ok().flatten() {
            let k = items.len() - 1;
            items[k] = last[..r.start()].to_string();
            items.push(last[r.end()..].to_string());
        }
    }
    if items.len() != n {
        return text.to_string();
    }
    let mut lead = trim_hspace(&text[..whole_end]).to_string();
    if lead.ends_with('.') {
        lead.pop();
        lead.push(':');
    }
    if !tail.is_empty() {
        tail = format!("\n{tail}");
    }
    let list: Vec<String> = items.iter().enumerate().map(|(k, it)| format!("{}. {it}", k + 1)).collect();
    format!("{lead}\n{}{tail}", list.join("\n"))
}
