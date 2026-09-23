//! OWNER: team C (engines). System prompt / user message builders, verbatim from Polisher.swift.
//!
//! The system prompt must be byte-identical to the Swift app's for the same inputs: Qwen3.5 is a hybrid
//! (linear-attention) model and llama-server can only resume from the checkpoint it keeps at the end of the
//! system prompt, so anything that varies per dictation travels in the user message instead.
//!
//! The long literals below were extracted mechanically from Polisher.swift (Swift `"""` blocks with their
//! indentation removed); do not edit them by hand.

use std::collections::BTreeMap;

use jez_core::field_context::FieldContext;
use jez_core::traits::Examples;
use once_cell::sync::Lazy;
use regex::Regex;
use unicode_segmentation::UnicodeSegmentation;

/// First `n` Characters (extended grapheme clusters), like Swift `String.prefix(n)`.
pub(crate) fn prefix_chars(s: &str, n: usize) -> &str {
    match s.grapheme_indices(true).nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// Last `n` Characters, like Swift `String.suffix(n)`.
pub(crate) fn suffix_chars(s: &str, n: usize) -> &str {
    if n == 0 {
        return "";
    }
    match s.grapheme_indices(true).rev().nth(n - 1) {
        Some((i, _)) => &s[i..],
        None => s,
    }
}

/// Swift `CharacterSet.whitespaces` (Unicode Zs plus tab); no newlines.
fn is_swift_whitespace(c: char) -> bool {
    c == '\t'
        || matches!(
            c,
            ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
        )
}

/// Swift `.trimmingCharacters(in: .whitespacesAndNewlines)`.
pub(crate) fn trim_ws_nl(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || c == '\u{85}')
}

/// `TextRules.parseDictionary`: "Term", "Term: a, b", "Term = a; b" or "Term (a, b)" per line.
/// Kept here (not in jez-core) because the prompt must match Swift byte for byte regardless of other changes.
pub fn parse_dictionary(lines: &[String]) -> Vec<(String, Vec<String>)> {
    static SEP: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s*[:=]\s*").unwrap());
    static PAREN: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s*\((.*)\)\s*$").unwrap());
    lines
        .iter()
        .filter_map(|line| {
            let l = line.trim_matches(is_swift_whitespace);
            if l.is_empty() {
                return None;
            }
            let (mut term, mut rest) = (l.to_string(), String::new());
            if let Some(m) = SEP.find(l) {
                term = l[..m.start()].to_string();
                rest = l[m.end()..].to_string();
            } else if let Some(m) = PAREN.find(l) {
                term = l[..m.start()].to_string();
                rest = l[m.start()..].trim_matches(|c| c == ' ' || c == '(' || c == ')').to_string();
            }
            let aliases = rest
                .split([',', ';'])
                .map(|a| a.trim_matches(is_swift_whitespace).to_string())
                .filter(|a| !a.is_empty())
                .collect();
            Some((term.trim_matches(is_swift_whitespace).to_string(), aliases))
        })
        .collect()
}

/// `Polisher.systemPrompt`. `learned_signals` is `config.learnedSignals`.
pub fn system_prompt(
    tones: &BTreeMap<String, String>,
    dictionary: &[String],
    memory: &str,
    examples: &Examples,
    learned_signals: &[String],
) -> String {
    let learned_signals_line = if learned_signals.is_empty() {
        String::new()
    } else {
        format!(
            "\nFrom this speaker's own corrections, these phrases are how their \"no wait\" gets heard: {}. Treat them as corrections when the sentence reads better that way.",
            learned_signals.iter().map(|s| format!("\"{s}\"")).collect::<Vec<_>>().join(", ")
        )
    };
    // BTreeMap iterates in sorted key order, like `tones.keys.sorted()`.
    let tone_list = tones
        .iter()
        .filter(|(_, instruction)| !instruction.is_empty())
        .map(|(name, instruction)| {
            let mut block = format!("### {name}\n{instruction}");
            if let Some(ex) = examples.get(name).filter(|e| !e.is_empty()) {
                block += &format!(
                    "\nThis speaker edited earlier {name} outputs; match their style exactly (transcript -> what they wanted):\n"
                );
                block += &ex[ex.len().saturating_sub(3)..]
                    .iter()
                    .map(|(heard, wanted)| format!("\"{}\" -> \"{}\"", prefix_chars(heard, 200), prefix_chars(wanted, 200)))
                    .collect::<Vec<_>>()
                    .join("\n");
            }
            block
        })
        .collect::<Vec<_>>()
        .join("\n");
    let learned = format!("\nTones (only the one named at the end applies):\n{tone_list}");
    let entries = parse_dictionary(dictionary);
    let dictionary = if entries.is_empty() {
        String::new()
    } else {
        let list = entries
            .iter()
            .map(|(term, aliases)| {
                if aliases.is_empty() {
                    term.clone()
                } else {
                    format!("{term} (sounds like: {})", aliases.join(", "))
                }
            })
            .collect::<Vec<_>>()
            .join("; ");
        format!("\nCustom vocabulary: {list}. If the transcript has a word or phrase that sounds like one of these, replace it with the term exactly as written here, even in a lowercase tone.")
    };
    let background = if trim_ws_nl(memory).is_empty() {
        String::new()
    } else {
        format!("\nAbout the speaker (background for interpreting names and context only; never insert it into the output): {memory}")
    };
    let mut s = String::with_capacity(SYSTEM_HEAD.len() + SYSTEM_TAIL.len() + 2048);
    s += SYSTEM_HEAD;
    s += &dictionary;
    s += &background;
    s += &learned;
    s += &learned_signals_line;
    s += SYSTEM_TAIL;
    s
}

/// `Polisher.userMessage`: the per-dictation bracket line (tone + field context), then the transcript.
pub fn user_message(raw: &str, tone: &str, context: Option<&FieldContext>) -> String {
    let mut meta = format!("[tone: {tone}");
    if let Some(c) = context.filter(|c| !c.is_empty()) {
        let clean = |s: &str| s.replace('\n', " ").replace('"', "'");
        if !c.window_title.is_empty() {
            meta += &format!(" | window: {}", clean(prefix_chars(&c.window_title, 60)));
        }
        if !c.before.is_empty() {
            meta += &format!(" | before: \"{}\"", clean(suffix_chars(&c.before, 160)));
        }
        if !c.after.is_empty() {
            meta += &format!(" | after: \"{}\"", clean(prefix_chars(&c.after, 100)));
        }
    }
    meta + "]\n" + raw
}

/// `meetingTitle` system prompt.
pub const TITLE_SYSTEM: &str = "Give these meeting notes a title of 3 to 7 words that says what the meeting was about, in the language of the notes, like a calendar event name (e.g. \"Pricing page launch and reel\"). Never start with \"Meeting\", \"Notes\" or \"Summary\". Output only the title, no quotes, no full stop.";

/// `summarizeMeeting` per-chunk system prompt. `memory` is `config.memory`.
pub fn chunk_system(memory: &str) -> String {
    let who = if trim_ws_nl(memory).is_empty() {
        String::new()
    } else {
        format!(
            " About that person: {} When the call mentions them by name, that is \"You\".",
            prefix_chars(memory, 400)
        )
    };
    format!("You take notes for the person whose lines are labelled \"You\"; \"Them\" is everyone else on the call.{who} Below is part of a meeting transcript.\n") + NOTES_FORMAT
}

/// `summarizeMeeting` merge system prompt.
pub fn merge_system() -> String {
    "Below are notes from consecutive parts of one meeting. Merge them into one set of notes, removing repetition and keeping every decision and action item.\n".to_string() + NOTES_FORMAT
}

/// Everything before `\(dictionary)\(background)\(learned)\(learnedSignals)` in `systemPrompt`.
pub(crate) const SYSTEM_HEAD: &str = "You clean up dictated speech-to-text transcripts. Work in two passes.

Pass 1 - self-corrections. Speakers abandon a phrase and restart. Signals: \"sorry\", \"no wait\", \"actually\", \"I mean\", \"scratch that\", \"correction\", or an abrupt restart of the sentence. Delete the abandoned phrase (everything from the start of that thought up to and including the signal word) and keep only the restated version. The abandoned phrase is usually unfinished and the restated version usually reuses some of its words.
Examples:
\"today's weather is very quiet sorry today's weather is very sunny\" -> \"today's weather is very sunny\"
\"I um I really want to um actually sorry, she doesn't really want to do it.\" -> \"She doesn't really want to do it.\"
\"send it to John no wait to Alex by Friday\" -> \"send it to Alex by Friday\"
\"we ship on tuesday actually thursday\" -> \"we ship on thursday\"
Speech recognition often garbles the signal itself: \"no wait\" comes out as \"I know\", \"no way\", \"no wake\" or \"no weight\"; \"sorry\" as \"so we\" or \"sore\"; \"scratch that\" as \"scratch it\". A short aside in the middle of a sentence that makes no sense as content (\"I'll do Vegemite toast, I know, KFC for lunch\") is a misheard correction: treat it as \"no wait\" and apply the replacement (\"I'll do KFC for lunch\").
A short fragment after the signal replaces the part of the sentence it stands in for, even when that part is not the last thing said:
\"I'll do grilled cheese for lunch, no wait, dino nuggets\" -> \"I'll do dino nuggets for lunch\"
\"I'll do grilled cheese, oh no, dino nuggets for lunch\" -> \"I'll do dino nuggets for lunch\"
\"meet me at the cafe at three, sorry, four\" -> \"meet me at the cafe at four\"
\"send the deck to Alex, I mean the budget\" -> \"send the budget to Alex\"
\"I caught a cold from my partner and she actually he was also sick\" -> \"I caught a cold from my partner and he was also sick\"
\"Look, she doesn't really want to do it. Sorry, I meant look she doesn't want to do it\" -> \"Look, she doesn't want to do it.\"
\"sorry I'm late, the train was um delayed\" -> \"sorry I'm late, the train was delayed\" (a real apology is kept; only the filler goes)
With \"scratch that\", \"forget that\" or \"never mind\", the speaker throws away the whole sentence or fragment before it. Delete all of it; the sentence after stands on its own and must not borrow any words from the deleted part.
\"Um tomorrow tomorrow we look at actually scratch that. Let's circle back on this.\" -> \"Let's circle back on this.\"
\"so the budget is scratch that we'll talk money later\" -> \"we'll talk money later\"

Pass 2 - cleanup. Remove every filler word (um, umm, uh, er, like, you know, I mean), stutters and accidental repeated words. Fix grammar and punctuation. Keep everything else exactly as the speaker said it: their wording, greetings, questions, swearing, laughter and reactions (haha, lol, oh, wow, ugh are content, not fillers), sentence order and length. Never add words, answer, summarize or comment.

Structure (all tones; use blank lines between blocks):
- The transcript may contain line breaks. Keep every line break exactly where it is and never merge lines.
- A greeting at the start (\"hi Alex\", \"dear all\", \"hey team\", \"morning\") goes on its own line, followed by a blank line.
- A sign-off at the end (\"kind regards\", \"cheers\", \"thanks\", \"best\", \"talk soon\", optionally followed by a name) goes after a blank line, on its own line, with the name on the next line: \"Kind regards\nTaylor\". In casual tone keep it lowercase and short.
- Start a new paragraph when the topic changes or after three or four sentences. Short chat replies stay one paragraph.
- A spoken count followed by the items (\"three things: it's fast, it's accurate, it gets names right\") is a numbered list too, in every tone: the lead-in ends with a colon, then one item per line.
- Lists: when the speaker enumerates (\"number one ... number two\", \"first ... second ... third\", \"point one\", \"a) b)\"), write a numbered list, one item per line, dropping the spoken markers. \"bullet point\", \"dot point\", \"next point\" or three or more parallel items listed in a row become a \"- \" bullet list. A lead-in like \"three things:\" stays on its own line before the list.
- Keep email addresses, URLs, numbers, times and dates exactly as written in the transcript. Emoji in the transcript are content: keep each one exactly where it is.
- Well-known companies, products, brands and public figures get their official spelling and capitalisation even when the transcript spells them phonetically: \"whisper flow\" -> Wispr Flow, \"chat gpt\" -> ChatGPT, \"i phone\" -> iPhone, \"you tube\" -> YouTube. Use the speaker's background to resolve names from their world.
- Questions end with a question mark, including statements asked as questions (\"you really think this is possible?\", \"that's the plan?\"). Never drop a question mark the transcript had.
- Put quotation marks around words or phrases the speaker is quoting, naming or mentioning as words, around titles, and around reported speech: words such as \"what\", \"when\" or \"how\"; she said \"no way\"; the \"Send\" button. This applies in every tone.
Example transcript: \"dear alex thanks for sending the deck over three things number one the pricing slide needs the aud figures number two can we drop the roadmap section and number three send the final to me by friday happy to jump on a call if easier kind regards taylor\"
Example output (professional):
Dear Alex,

Thanks for sending the deck over. Three things:
1. The pricing slide needs the AUD figures.
2. Can we drop the roadmap section?
3. Send the final to me by Friday.

Happy to jump on a call if easier.

Kind regards
Taylor
";

/// Everything after the interpolation line in `systemPrompt`.
pub(crate) const SYSTEM_TAIL: &str = "

Each message is one dictation in this exact form:
[tone: NAME | window: TITLE | before: TEXT | after: TEXT]
<transcript>
The bracket line is metadata: which tone to apply, and where the text will be inserted (context only; never repeat or answer it). If \"before\" ends mid-sentence, the output continues that sentence: no capital first letter, no greeting. Reply with the cleaned transcript only. Never output the bracket line or any label.
Example message:
[tone: professional | window: Alex — Messages]
um hi alex so the deck is is ready i'll send it by friday
Example reply:
Hi Alex, the deck is ready. I'll send it by Friday.
Example message:
[tone: professional | window: Re: Q3 numbers — Mail | before: \"Hi Alex, about the\"]
um numbers they look great and we should ship on thursday
Example reply:
numbers, they look great and we should ship on Thursday.";

/// `Polisher.notesFormat`.
pub const NOTES_FORMAT: &str = "Write the notes in the language the meeting was held in. Use exactly this markdown layout and nothing else:
## Summary
- 3 to 6 bullets covering what was discussed and concluded
## Decisions
- one bullet per decision (write \"- None\" if there were none)
## Action items
- one bullet per task: who, what, by when if said (write \"- None\" if there were none)
## Your to-dos
- only the tasks the person labelled \"You\" committed to or was asked to do, one per bullet, starting with a verb, with the deadline if said (write \"- None\" if there were none)
Be concrete: keep names, numbers, dates and product names. Never invent anything that is not in the transcript.";
