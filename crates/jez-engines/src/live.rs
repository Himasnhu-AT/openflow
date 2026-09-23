//! OWNER: team D (sidecar). Live re-decode with freezing over any AsrEngine (Transcriber.swift liveUpdate/liveFinish).
//!
//! Parakeet decodes 15 s in ~150 ms, so the live scheme simply re-decodes everything said so far every few
//! hundred ms. Re-decoding more than ~8 s each time made long dictations lag, so once 8 s have accumulated past
//! the frozen point, the audio up to the quietest 200 ms between 5 s and 7.5 s is decoded once more WITH
//! vocabulary rescoring (exactly like a batch result) and frozen; decoding continues from there.

use std::sync::Arc;

use jez_core::traits::{AsrEngine, Result};
use tokio::sync::Mutex;

/// Freeze once this much audio has accumulated past the frozen point (8 s).
const FREEZE_AFTER: usize = 8 * 16_000;
/// Where the freeze may land, relative to the frozen point (5 s ..= 7.5 s).
const FREEZE_SEARCH_START: usize = 5 * 16_000;
const FREEZE_SEARCH_END: usize = 7 * 16_000 + 8_000;
/// Quiet-point search window (200 ms) and step (50 ms).
const WINDOW: usize = 3_200;
const STEP: usize = 800;
/// Swift's `String.count >= 4` (boost terms shorter than this are dropped).
const MIN_TERM_CHARS: usize = 4;

#[derive(Default)]
struct LiveState {
    frozen_text: Vec<String>,
    frozen_samples: usize,
    boosted_terms: Vec<String>,
}

/// Live dictation text plus the dictionary/extra-name vocabulary merge of Transcriber.swift.
/// Calls are serialized (the Swift actor), so an update never races a finish.
pub struct LiveTranscriber {
    asr: Arc<dyn AsrEngine>,
    state: Mutex<LiveState>,
}

impl LiveTranscriber {
    pub fn new(asr: Arc<dyn AsrEngine>) -> Self {
        LiveTranscriber {
            asr,
            state: Mutex::new(LiveState::default()),
        }
    }

    pub fn engine(&self) -> &Arc<dyn AsrEngine> {
        &self.asr
    }

    /// Forget any frozen text (hotkey down).
    pub async fn begin_live(&self) {
        let mut s = self.state.lock().await;
        s.frozen_text.clear();
        s.frozen_samples = 0;
    }

    /// Best current guess for everything said so far. `samples` = all audio since the hotkey went down.
    /// Empty while the model is still loading.
    pub async fn live_update(&self, samples: &[f32]) -> Result<String> {
        if !self.asr.is_ready() {
            return Ok(String::new());
        }
        let mut s = self.state.lock().await;
        if s.frozen_samples > samples.len() {
            // A new recording without begin_live: start over rather than index past the end.
            s.frozen_text.clear();
            s.frozen_samples = 0;
        }
        if samples.len() - s.frozen_samples > FREEZE_AFTER {
            let cut = s.frozen_samples + quietest_point(samples, s.frozen_samples);
            let text = self
                .asr
                .transcribe(&samples[s.frozen_samples..cut], true)
                .await?;
            if !text.is_empty() {
                s.frozen_text.push(text);
            }
            s.frozen_samples = cut;
        }
        let tail = &samples[s.frozen_samples..];
        let tail = if tail.is_empty() {
            String::new()
        } else {
            self.asr.transcribe(tail, false).await?
        };
        Ok(join(&s.frozen_text, tail.trim()))
    }

    /// Final transcript: frozen parts plus the rescored tail (only the tail is decoded after release). Resets.
    pub async fn live_finish(&self, samples: &[f32]) -> Result<String> {
        let mut s = self.state.lock().await;
        let result = if s.frozen_samples < samples.len() {
            self.asr
                .transcribe(&samples[s.frozen_samples..], true)
                .await
        } else {
            Ok(String::new())
        };
        let text = result.map(|tail| join(&s.frozen_text, &tail));
        s.frozen_text.clear();
        s.frozen_samples = 0;
        text
    }

    /// Biases recognition toward the dictionary terms (the part of each line before ':' / '=' / '(') plus
    /// `extra` names (the field being typed in, Contacts, Calendar). No-op when the merged list is unchanged.
    pub async fn set_vocabulary(&self, dictionary: &[String], extra: &[String]) -> Result<()> {
        let terms = jez_core::text_rules::dictionary_terms(dictionary);
        self.set_boost_terms(terms, extra).await
    }

    /// `set_vocabulary` after the dictionary lines were reduced to terms.
    async fn set_boost_terms(&self, terms: Vec<String>, extra: &[String]) -> Result<()> {
        let cleaned = merge_vocabulary(terms, extra);
        let mut s = self.state.lock().await;
        if cleaned == s.boosted_terms {
            return Ok(());
        }
        s.boosted_terms = cleaned.clone();
        self.asr.set_vocabulary(&cleaned).await
    }
}

/// Dictionary terms plus `extra` names not already present (case-insensitive), each trimmed, keeping those with
/// at least 4 characters. Order: terms first, then new extras, as in Transcriber.setVocabulary.
pub fn merge_vocabulary(mut names: Vec<String>, extra: &[String]) -> Vec<String> {
    for n in extra {
        let lower = n.to_lowercase();
        if !names.iter().any(|x| x.to_lowercase() == lower) {
            names.push(n.clone());
        }
    }
    names
        .iter()
        .map(|n| trim_whitespaces(n))
        .filter(|n| n.chars().count() >= MIN_TERM_CHARS)
        .map(str::to_owned)
        .collect()
}

/// `trimmingCharacters(in: .whitespaces)`: spaces and tabs, not newlines.
fn trim_whitespaces(s: &str) -> &str {
    s.trim_matches(|c: char| {
        c.is_whitespace()
            && !matches!(
                c,
                '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}'
            )
    })
}

fn join(frozen: &[String], tail: &str) -> String {
    frozen
        .iter()
        .map(String::as_str)
        .chain(std::iter::once(tail))
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Start (relative to `offset`) of the quietest 200 ms window in 5 s ..= 7.5 s past `offset`, so freezes land
/// between words. Minimum Σx²; the first minimum wins.
fn quietest_point(samples: &[f32], offset: usize) -> usize {
    let mut best = FREEZE_SEARCH_START;
    let mut best_energy = f32::MAX;
    let mut i = FREEZE_SEARCH_START;
    while i + WINDOW <= FREEZE_SEARCH_END && offset + i + WINDOW <= samples.len() {
        let e: f32 = samples[offset + i..offset + i + WINDOW]
            .iter()
            .map(|x| x * x)
            .sum();
        if e < best_energy {
            best_energy = e;
            best = i;
        }
        i += STEP;
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex as StdMutex;

    /// Returns "<n>" for a decode of n samples (or "" for n == 0) and records every call.
    #[derive(Default)]
    struct FakeAsr {
        not_ready: AtomicBool,
        calls: StdMutex<Vec<(usize, bool)>>,
        vocab: StdMutex<Vec<Vec<String>>>,
    }

    #[async_trait]
    impl AsrEngine for FakeAsr {
        async fn load(&self, _version: &str) -> Result<()> {
            Ok(())
        }
        fn is_ready(&self) -> bool {
            !self.not_ready.load(Ordering::SeqCst)
        }
        async fn transcribe(&self, samples: &[f32], rescore: bool) -> Result<String> {
            self.calls.lock().unwrap().push((samples.len(), rescore));
            Ok(if samples.is_empty() {
                String::new()
            } else {
                format!("{}{}", if rescore { "R" } else { "t" }, samples.len())
            })
        }
        async fn set_vocabulary(&self, terms: &[String]) -> Result<()> {
            self.vocab.lock().unwrap().push(terms.to_vec());
            Ok(())
        }
        async fn transcribe_multilingual(&self, _samples: &[f32]) -> Result<String> {
            Ok(String::new())
        }
    }

    fn setup() -> (Arc<FakeAsr>, LiveTranscriber) {
        let fake = Arc::new(FakeAsr::default());
        let live = LiveTranscriber::new(fake.clone());
        (fake, live)
    }

    /// Loud audio of `len` samples with a silent 200 ms window starting at `quiet`.
    fn audio(len: usize, quiet: Option<usize>) -> Vec<f32> {
        (0..len)
            .map(|i| match quiet {
                Some(q) if (q..q + WINDOW).contains(&i) => 0.0,
                _ => {
                    if i % 2 == 0 {
                        0.3
                    } else {
                        -0.3
                    }
                }
            })
            .collect()
    }

    #[tokio::test]
    async fn short_audio_decodes_tail_without_rescore() {
        let (fake, live) = setup();
        let text = live.live_update(&audio(5 * 16_000, None)).await.unwrap();
        assert_eq!(text, "t80000");
        assert_eq!(*fake.calls.lock().unwrap(), vec![(80_000, false)]);
        // exactly 8 s does not freeze yet
        live.live_update(&audio(FREEZE_AFTER, None)).await.unwrap();
        assert_eq!(
            fake.calls.lock().unwrap().last(),
            Some(&(FREEZE_AFTER, false))
        );
    }

    #[tokio::test]
    async fn freezes_at_quietest_window_with_rescore() {
        let (fake, live) = setup();
        let samples = audio(9 * 16_000, Some(96_000));
        let text = live.live_update(&samples).await.unwrap();
        assert_eq!(text, "R96000 t48000");
        assert_eq!(
            *fake.calls.lock().unwrap(),
            vec![(96_000, true), (48_000, false)]
        );
    }

    #[tokio::test]
    async fn second_freeze_is_relative_to_frozen_offset() {
        let (fake, live) = setup();
        let mut samples = audio(9 * 16_000, Some(96_000));
        live.live_update(&samples).await.unwrap(); // frozen at 96000
                                                   // grow to 96000 + 8 s + 1 with a quiet window 6.5 s past the frozen point
        samples = audio(96_000 + FREEZE_AFTER + 1, Some(96_000 + 104_000));
        samples[..96_000].iter_mut().for_each(|x| *x = 0.3);
        fake.calls.lock().unwrap().clear();
        let text = live.live_update(&samples).await.unwrap();
        assert_eq!(text, "R96000 R104000 t24001");
        assert_eq!(
            *fake.calls.lock().unwrap(),
            vec![(104_000, true), (24_001, false)]
        );
    }

    #[tokio::test]
    async fn no_quiet_window_freezes_at_search_start() {
        let (_fake, live) = setup();
        // constant loudness: every window ties, the first (5 s) wins
        let text = live.live_update(&audio(9 * 16_000, None)).await.unwrap();
        assert_eq!(text, "R80000 t64000");
    }

    #[tokio::test]
    async fn finish_rescores_tail_joins_and_resets() {
        let (fake, live) = setup();
        let samples = audio(9 * 16_000, Some(96_000));
        live.live_update(&samples).await.unwrap();
        fake.calls.lock().unwrap().clear();
        let text = live.live_finish(&samples).await.unwrap();
        assert_eq!(text, "R96000 R48000");
        assert_eq!(*fake.calls.lock().unwrap(), vec![(48_000, true)]);
        // reset: the next session starts from zero
        fake.calls.lock().unwrap().clear();
        assert_eq!(
            live.live_update(&audio(16_000, None)).await.unwrap(),
            "t16000"
        );
    }

    #[tokio::test]
    async fn finish_without_updates_and_with_nothing_left() {
        let (fake, live) = setup();
        assert_eq!(
            live.live_finish(&audio(32_000, None)).await.unwrap(),
            "R32000"
        );
        assert_eq!(live.live_finish(&[]).await.unwrap(), "");
        assert_eq!(fake.calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn begin_live_resets_frozen_text() {
        let (_fake, live) = setup();
        live.live_update(&audio(9 * 16_000, Some(96_000)))
            .await
            .unwrap();
        live.begin_live().await;
        assert_eq!(
            live.live_update(&audio(16_000, None)).await.unwrap(),
            "t16000"
        );
    }

    #[tokio::test]
    async fn not_ready_returns_empty() {
        let (fake, live) = setup();
        fake.not_ready.store(true, Ordering::SeqCst);
        assert_eq!(live.live_update(&audio(16_000, None)).await.unwrap(), "");
        assert!(fake.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn merge_dedupes_trims_and_filters() {
        let terms = vec![
            "JezFlow".to_string(),
            " Zentro ".to_string(),
            "abc".to_string(),
        ];
        let extra = vec![
            "jezflow".to_string(),
            "Himanshu".to_string(),
            "Al".to_string(),
            "zentro ".to_string(),
        ];
        // "zentro " differs from " Zentro " before trimming (Swift compares untrimmed), so it is appended and
        // both survive, as in Transcriber.setVocabulary.
        assert_eq!(
            merge_vocabulary(terms, &extra),
            vec!["JezFlow", "Zentro", "Himanshu", "zentro"]
        );
        assert!(merge_vocabulary(vec![], &[]).is_empty());
    }

    #[tokio::test]
    async fn boost_terms_skip_unchanged_and_empty_disables() {
        let (fake, live) = setup();
        live.set_boost_terms(vec!["JezFlow".into()], &["Himanshu".into()])
            .await
            .unwrap();
        live.set_boost_terms(
            vec!["JezFlow".into()],
            &["Himanshu".into(), "himanshu".into()],
        )
        .await
        .unwrap();
        live.set_boost_terms(vec![], &[]).await.unwrap();
        live.set_boost_terms(vec!["abc".into()], &[]).await.unwrap(); // filters to empty again: unchanged
        assert_eq!(
            *fake.vocab.lock().unwrap(),
            vec![vec!["JezFlow".to_string(), "Himanshu".to_string()], vec![]]
        );
    }

    #[tokio::test]
    async fn set_vocabulary_uses_dictionary_terms() {
        let (fake, live) = setup();
        live.set_vocabulary(
            &["Zentro: zentrow, sentro".into(), "Kubernetes".into()],
            &["Himanshu".into()],
        )
        .await
        .unwrap();
        assert_eq!(
            *fake.vocab.lock().unwrap(),
            vec![vec![
                "Zentro".to_string(),
                "Kubernetes".to_string(),
                "Himanshu".to_string()
            ]]
        );
    }
}
