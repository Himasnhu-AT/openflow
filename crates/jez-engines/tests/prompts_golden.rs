//! Prompt parity with the Swift app. `fixtures/prompt_golden.json` was produced by the real Swift code:
//! `Polisher.systemPrompt`, `userMessage`, the notes prompts and `TextRules.parseDictionary` were extracted by
//! line range from dep/Sources/JezFlow/*.swift into a harness and run on `fixtures/prompt_cases.json`.
//! Any difference here breaks llama-server prompt caching parity, so compare bytes, not "close enough".

use std::collections::BTreeMap;

use jez_core::field_context::FieldContext;
use jez_core::traits::Examples;
use jez_engines::prompts;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    tones: BTreeMap<String, String>,
    dictionary: Vec<String>,
    memory: String,
    learned_signals: Vec<String>,
    examples: BTreeMap<String, Vec<(String, String)>>,
    raw: String,
    tone: String,
    context: Option<BTreeMap<String, String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Golden {
    name: String,
    system: String,
    user: String,
    chunk_system: String,
    merge_system: String,
    title_system: String,
}

fn load() -> Vec<(Case, Golden)> {
    let cases: Vec<Case> = serde_json::from_str(include_str!("fixtures/prompt_cases.json")).unwrap();
    let golden: Vec<Golden> = serde_json::from_str(include_str!("fixtures/prompt_golden.json")).unwrap();
    assert_eq!(cases.len(), golden.len());
    cases.into_iter().zip(golden).collect()
}

fn first_diff(a: &str, b: &str) -> String {
    let i = a.bytes().zip(b.bytes()).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
    let lo = i.saturating_sub(60);
    format!(
        "first difference at byte {i}:\n rust: {:?}\nswift: {:?}",
        String::from_utf8_lossy(&a.as_bytes()[lo..(i + 60).min(a.len())]),
        String::from_utf8_lossy(&b.as_bytes()[lo..(i + 60).min(b.len())])
    )
}

#[test]
fn system_and_user_prompts_match_swift() {
    for (c, g) in load() {
        assert_eq!(c.name, g.name);
        let examples: Examples = c.examples.clone();
        let system = prompts::system_prompt(&c.tones, &c.dictionary, &c.memory, &examples, &c.learned_signals);
        assert!(system == g.system, "{}: system prompt differs; {}", c.name, first_diff(&system, &g.system));
        let ctx = c.context.as_ref().map(|m| FieldContext {
            window_title: m.get("windowTitle").cloned().unwrap_or_default(),
            before: m.get("before").cloned().unwrap_or_default(),
            after: m.get("after").cloned().unwrap_or_default(),
            cursor: None,
        });
        let user = prompts::user_message(&c.raw, &c.tone, ctx.as_ref());
        assert!(user == g.user, "{}: user message differs; {}", c.name, first_diff(&user, &g.user));
    }
}

#[test]
fn notes_prompts_match_swift() {
    for (c, g) in load() {
        assert_eq!(prompts::chunk_system(&c.memory), g.chunk_system, "{}", c.name);
        assert_eq!(prompts::merge_system(), g.merge_system);
        assert_eq!(prompts::TITLE_SYSTEM, g.title_system);
    }
}
