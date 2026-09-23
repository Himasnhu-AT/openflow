//! OWNER: team A (rules). Mirrors Pipeline.swift: raw transcript -> text that gets pasted.

use once_cell::sync::Lazy;
use regex::Regex;

use crate::text_rules as rules;
use crate::traits::{Examples, PolishBackend};
use crate::{Config, FieldContext};

static SIGNALS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(sorry|actually|no[,\s]+wait|wait[,\s]+no|oh[,\s]+no|i mean|scratch that|forget that|never ?mind|correction|number (one|two|1|2)|first(ly)?|second(ly)?)\b").expect("regex")
});
static CORRECTIONS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(sorry|no[,\s]+wait|wait[,\s]+no|oh[,\s]+no|i mean|scratch that|forget that|never ?mind|correction|actually|let me rephrase)\b").expect("regex")
});
/// A one or two word aside set off by commas ("..., I know, ...") is often a misheard correction; it counts as a
/// signal so the model gets to decide.
static ASIDE: Lazy<Regex> = Lazy::new(|| Regex::new(r",\s*[A-Za-z']+(?:\s+[A-Za-z']+)?\s*,").expect("regex"));

pub fn has_correction_signal(text: &str, learned: &[String]) -> bool {
    if CORRECTIONS.is_match(text) || ASIDE.is_match(text) {
        return true;
    }
    let lower = format!(" {} ", text.to_lowercase());
    learned.iter().any(|s| lower.contains(&format!(" {s} ")) || lower.contains(&format!(" {s},")))
}

/// Shared by the app and `--eval` so a replay scores exactly what a real dictation would have produced.
pub async fn run(
    raw: &str,
    tone: &str,
    config: &Config,
    polisher: Option<&dyn PolishBackend>,
    examples: &Examples,
    context: Option<&FieldContext>,
    on_partial: Option<&(dyn Fn(&str) + Send + Sync)>,
) -> anyhow::Result<String> {
    let traditional = config.chinese_script == "traditional";
    // Spoken commands ("new line", "sam at example dot com") apply in every tone, verbatim included.
    let text = rules::chinese_script(&rules::apply_dictionary(&rules::apply_commands(raw), &config.dictionary), traditional);
    let instruction = config.tones.get(tone).map(String::as_str).unwrap_or("");
    let Some(polisher) = polisher.filter(|_| !instruction.is_empty()) else { return Ok(text) };
    let pre = rules::strip_fillers(&text);
    let terms = rules::dictionary_terms(&config.dictionary);
    // Fast path: a tiny casual message with nothing to reason about needs no model.
    if (tone == "casual" || tone == "supercasual")
        && pre.split(' ').filter(|w| !w.is_empty()).count() <= 4
        && !pre.contains('\n')
        && !SIGNALS.is_match(&pre)
        && !has_correction_signal(&text, &config.learned_signals)
    {
        let quick = rules::question_marks(&rules::apply_counted_lists(&rules::apply_lists(&rules::layout_message(
            &rules::casual(&pre, &terms, tone == "supercasual"),
        ))));
        if let Some(cb) = on_partial {
            cb(&quick);
        }
        return Ok(quick);
    }
    let polished = polisher
        .polish(&pre, tone, &config.tones, &config.dictionary, &config.memory, examples, context, on_partial)
        .await?;
    if rules::looks_unfaithful(&pre, &polished, &terms) {
        tracing::error!(target: "pipeline", "Guardrail: model output rejected, pasting cleaned transcript instead. Output was: {polished}");
        return Ok(pre);
    }
    let mut out = rules::chinese_script(&rules::apply_dictionary(&polished, &config.dictionary), traditional);
    if let Some(before) = context.map(|c| c.before.as_str()).filter(|b| !b.is_empty()) {
        out = rules::strip_echo(&out, before);
    }
    Ok(rules::question_marks(&rules::apply_counted_lists(&rules::apply_lists(&rules::layout_message(
        &rules::layout_greeting(&out),
    )))))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use async_trait::async_trait;

    use super::*;

    /// Returns a canned reply and records what it was asked to polish.
    struct FakePolisher {
        reply: String,
        seen: Mutex<Vec<String>>,
    }

    impl FakePolisher {
        fn new(reply: &str) -> Self {
            Self { reply: reply.into(), seen: Mutex::new(vec![]) }
        }
        fn calls(&self) -> Vec<String> {
            self.seen.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl PolishBackend for FakePolisher {
        async fn polish(
            &self,
            raw: &str,
            _tone: &str,
            _tones: &BTreeMap<String, String>,
            _dictionary: &[String],
            _memory: &str,
            _examples: &Examples,
            _context: Option<&FieldContext>,
            on_partial: Option<&(dyn for<'s> Fn(&'s str) + Send + Sync)>,
        ) -> anyhow::Result<String> {
            self.seen.lock().unwrap().push(raw.to_string());
            if let Some(f) = on_partial {
                let partial = self.reply.chars().take(3).collect::<String>();
                f(&partial);
            }
            Ok(self.reply.clone())
        }
    }

    fn config() -> Config {
        Config {
            tones: BTreeMap::from([
                ("verbatim".to_string(), String::new()),
                ("casual".to_string(), "lowercase".to_string()),
                ("supercasual".to_string(), "lowercase, no punctuation".to_string()),
                ("formal".to_string(), "proper".to_string()),
            ]),
            dictionary: vec!["Acme Hub: acme hub".into()],
            learned_signals: vec!["i know".into()],
            chinese_script: "simplified".into(),
            ..Config::default()
        }
    }

    fn block<F: std::future::Future>(f: F) -> F::Output {
        // The pipeline never awaits anything but the polisher, which is ready immediately in these tests.
        let waker = std::task::Waker::noop();
        let mut cx = std::task::Context::from_waker(waker);
        let mut f = std::pin::pin!(f);
        loop {
            if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }

    async fn go(raw: &str, tone: &str, p: Option<&FakePolisher>, context: Option<&FieldContext>) -> String {
        let cfg = config();
        run(raw, tone, &cfg, p.map(|p| p as &dyn PolishBackend), &Examples::new(), context, None).await.unwrap()
    }

    #[test]
    fn signals() {
        assert!(has_correction_signal("send it monday, actually friday", &[]));
        assert!(has_correction_signal("toast, i know, kfc", &[]));
        assert!(has_correction_signal("vegemite toast i know kfc", &["i know".into()]));
        assert!(!has_correction_signal("see you at eight", &["i know".into()]));
    }

    #[test]
    fn verbatim_and_no_polisher_only_apply_commands() {
        let p = FakePolisher::new("SHOULD NOT BE USED");
        assert_eq!(block(go("um the acme hub report new line thanks", "verbatim", Some(&p), None)), "um the Acme Hub report\nthanks");
        assert_eq!(block(go("um the acme hub report", "formal", None, None)), "um the Acme Hub report");
        assert!(p.calls().is_empty());
    }

    #[test]
    fn fast_path_skips_model() {
        let p = FakePolisher::new("SHOULD NOT BE USED");
        let seen = Mutex::new(String::new());
        let cb = |s: &str| *seen.lock().unwrap() = s.to_string();
        let cfg = config();
        let out = block(run("Um, See you at eight.", "casual", &cfg, Some(&p), &Examples::new(), None, Some(&cb))).unwrap();
        assert_eq!(out, "see you at eight.");
        assert_eq!(*seen.lock().unwrap(), "see you at eight.");
        assert_eq!(block(go("Sounds good, see you.", "supercasual", Some(&p), None)), "sounds good see you");
        assert_eq!(block(go("are you coming", "casual", Some(&p), None)), "are you coming?");
        assert!(p.calls().is_empty());
    }

    #[test]
    fn signals_and_long_text_reach_the_model() {
        let p = FakePolisher::new("ok friday");
        assert_eq!(block(go("ok monday sorry friday", "casual", Some(&p), None)), "ok friday");
        let p2 = FakePolisher::new("toast i know kfc");
        assert_eq!(block(go("toast i know kfc", "casual", Some(&p2), None)), "toast i know kfc");
        assert_eq!(p2.calls(), vec!["toast i know kfc".to_string()]);
    }

    #[test]
    fn model_output_post_processed() {
        let raw = "um hi alex thanks for the acme hub deck it looks great cheers sam";
        let p = FakePolisher::new("Hi Alex, thanks for the acme hub deck. It looks great. Cheers Sam");
        let out = block(go(raw, "formal", Some(&p), None));
        assert_eq!(out, "Hi Alex,\n\nthanks for the Acme Hub deck. It looks great.\n\nCheers\nSam");
        assert_eq!(p.calls(), vec!["hi alex thanks for the Acme Hub deck it looks great cheers sam".to_string()]);
    }

    #[test]
    fn guardrail_falls_back_to_cleaned_transcript() {
        let raw = "okay so um the plan is we ship on thursday and then we do the reel for instagram";
        let p = FakePolisher::new("Sure! Here is a cleaned up version of your message about shipping.");
        assert_eq!(
            block(go(raw, "formal", Some(&p), None)),
            "okay so the plan is we ship on thursday and then we do the reel for instagram"
        );
    }

    #[test]
    fn echo_of_text_before_cursor_is_dropped() {
        let ctx = FieldContext { before: "thanks this is working now but".into(), ..Default::default() };
        let raw = "this is working now but we are missing paragraphs today";
        let p = FakePolisher::new("this is working now but we are missing paragraphs today.");
        assert_eq!(block(go(raw, "formal", Some(&p), Some(&ctx))), "we are missing paragraphs today.");
    }
}
