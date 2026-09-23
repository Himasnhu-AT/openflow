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

/// `JezFlow --dumpprompt casual` from the real Swift binary, run against `fixtures/dump_config.json`
/// (Swift defaults plus memory, learned signals, an extra tone and a parenthesised dictionary line).
/// Also checks that jez-core's default tone texts match the Swift ones byte for byte.
#[test]
fn dumpprompt_matches_swift_binary() {
    let config: jez_core::Config = serde_json::from_str(include_str!("fixtures/dump_config.json")).unwrap();
    let system = prompts::system_prompt(&config.tones, &config.dictionary, &config.memory, &Examples::new(), &config.learned_signals);
    let got = format!("{system}\n=====USER=====\n{}\n", prompts::user_message("TRANSCRIPT", "casual", None));
    let want = include_str!("fixtures/dumpprompt_casual.txt");
    assert!(got == want, "{}", first_diff(&got, want));
}

#[test]
fn default_config_tones_match_swift_defaults() {
    let swift: jez_core::Config = serde_json::from_str(include_str!("fixtures/dump_config.json")).unwrap();
    let rust = jez_core::Config::default();
    for name in ["casual", "professional", "supercasual", "verbatim"] {
        assert_eq!(rust.tones.get(name), swift.tones.get(name), "{name}");
    }
}
