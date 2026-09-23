//! OWNER: team C (engines). Per-utterance speech routing for meetings (MeetingSession.swift MeetingEngine).
//!
//! Cloud on: Groq Whisper for both sides. Otherwise the far side ("Them") goes to local Whisper large v3 turbo
//! when it is running (compressed call audio, any language); your own mic goes to Parakeet, with Whisper or
//! SenseVoice consulted only for longer utterances that don't read as English.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::Result;
use jez_core::text_rules::looks_english;
use jez_core::traits::AsrEngine;
use jez_core::Config;

use crate::cloud::{Cloud, Scope};
use crate::prompts::trim_ws_nl;
use crate::whisper_server::WhisperServer;

/// 300 ms of silence appended so the last word isn't clipped.
pub const TAIL_SAMPLES: usize = 4800;

pub struct MeetingEngine {
    asr: Arc<dyn AsrEngine>,
    config: Config,
    cloud: Option<Cloud>,
    whisper: Option<Arc<WhisperServer>>,
    /// Set by `load()` (Swift: `sense != nil`); SenseVoice itself loads lazily in the ASR engine.
    sense: AtomicBool,
}

/// Mostly non-Latin letters (CJK, Hangul, kana, Cyrillic, Thai…) and at least three of them.
/// Note: Swift counts every scalar above U+2E7F (not only letters), and compares with alphabetic scalars.
pub fn is_non_latin(t: &str) -> bool {
    let non_latin = t.chars().filter(|c| *c as u32 > 0x2E7F).count();
    non_latin >= 3 && non_latin * 2 >= t.chars().filter(|c| c.is_alphabetic()).count()
}

/// Call audio arrives quiet; lift it moderately. Never more than 4x: amplifying near-silence turns line noise
/// into "speech" and Whisper then invents sentences for it.
pub fn normalised(s: &[f32]) -> Vec<f32> {
    let peak = s.iter().fold(0f32, |m, x| m.max(x.abs()));
    if !(peak > 0.02 && peak < 0.25) {
        return s.to_vec();
    }
    let g = (0.5 / peak).min(4.0);
    s.iter().map(|x| x * g).collect()
}

pub fn rms(s: &[f32]) -> f32 {
    (s.iter().map(|x| x * x).sum::<f32>() / s.len().max(1) as f32).sqrt()
}

impl MeetingEngine {
    pub fn new(asr: Arc<dyn AsrEngine>, config: Config, whisper: Option<Arc<WhisperServer>>) -> Self {
        let cloud = Cloud::new(&config, Scope::Meetings, false);
        Self { asr, config, cloud, whisper, sense: AtomicBool::new(false) }
    }

    /// Enables the SenseVoice fallback unless the cloud, whisper or the English-only engine make it unused.
    pub async fn load(&self) -> Result<()> {
        if self.cloud.is_some() || self.whisper.is_some() || self.config.meeting_engine == "english" {
            return Ok(());
        }
        self.sense.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Returns the text and whether it is English.
    pub async fn transcribe(&self, samples: &[f32], speaker: &str) -> Result<(String, bool)> {
        // Too quiet to be a voice on the line: the segmenter saw energy, but not enough to trust.
        if speaker == "Them" && rms(samples) < 0.004 {
            return Ok((String::new(), true));
        }
        let mut audio = normalised(samples);
        audio.resize(audio.len() + TAIL_SAMPLES, 0.0);
        if let Some(cloud) = &self.cloud {
            // Swift passes `dictionaryTerms` as a prompt, which Cloud.transcribe never sends; omitted here.
            match cloud.transcribe(&audio, None).await {
                Ok(t) => {
                    let english = !is_non_latin(&t);
                    return Ok((t, english));
                }
                Err(e) => tracing::error!(target: "meeting", "Cloud speech failed, using local: {e}"),
            }
        }
        if speaker == "Them" {
            if let Some(w) = &self.whisper {
                match w.transcribe(&audio).await {
                    Ok((text, lang)) => {
                        let english = if lang.is_empty() { !is_non_latin(&text) } else { lang.starts_with("en") };
                        return Ok((text, english));
                    }
                    Err(e) => tracing::error!(target: "meeting", "whisper-server failed, using Parakeet: {e}"),
                }
            }
        }
        let english = self.asr.transcribe(&audio, true).await?;
        if self.config.meeting_engine == "english" {
            return Ok((english, true));
        }
        let seconds = samples.len() as f64 / 16_000.0;
        if seconds < 1.5 || looks_english(&english) {
            return Ok((english, true));
        }
        if let Some(w) = &self.whisper {
            if let Ok((text, lang)) = w.transcribe(&audio).await {
                let is_en = lang.starts_with("en") || (lang.is_empty() && !is_non_latin(&text));
                return Ok(if is_en { (english, true) } else { (text, false) });
            }
        }
        if !self.sense.load(Ordering::SeqCst) {
            return Ok((english, true));
        }
        let other = self.asr.transcribe_multilingual(&audio).await?;
        let other = trim_ws_nl(&other).to_string();
        Ok(if is_non_latin(&other) { (other, false) } else { (english, true) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;

    struct FakeAsr {
        english: String,
        multi: String,
        seen: Mutex<Vec<(usize, bool, &'static str)>>,
    }

    #[async_trait]
    impl AsrEngine for FakeAsr {
        async fn load(&self, _: &str) -> Result<()> {
            Ok(())
        }
        fn is_ready(&self) -> bool {
            true
        }
        async fn transcribe(&self, s: &[f32], rescore: bool) -> Result<String> {
            self.seen.lock().unwrap().push((s.len(), rescore, "parakeet"));
            Ok(self.english.clone())
        }
        async fn set_vocabulary(&self, _: &[String]) -> Result<()> {
            Ok(())
        }
        async fn transcribe_multilingual(&self, s: &[f32]) -> Result<String> {
            self.seen.lock().unwrap().push((s.len(), false, "sense"));
            Ok(self.multi.clone())
        }
    }

    fn engine(english: &str, multi: &str, meeting_engine: &str) -> (MeetingEngine, Arc<FakeAsr>) {
        let asr = Arc::new(FakeAsr { english: english.into(), multi: multi.into(), seen: Mutex::new(vec![]) });
        let cfg = Config { meeting_engine: meeting_engine.into(), cloud_enabled: false, ..Config::default() };
        (MeetingEngine::new(asr.clone(), cfg, None), asr)
    }

    #[test]
    fn non_latin() {
        assert!(!is_non_latin("hello world"));
        assert!(is_non_latin("你好世界"));
        assert!(!is_non_latin("你好"));
        // Cyrillic (U+0400..) is below U+2E7F, so the Swift rule does not count it despite its doc comment.
        assert!(!is_non_latin("Привет мир"));
        assert!(is_non_latin("こんにちは hi"));
        assert!(!is_non_latin("hello there my friend 你好世"));
    }

    #[test]
    fn normalise_rules() {
        assert_eq!(normalised(&[0.01, -0.02]), vec![0.01, -0.02]); // peak 0.02: not > 0.02
        assert_eq!(normalised(&[0.3, 0.1]), vec![0.3, 0.1]); // loud enough
        assert_eq!(normalised(&[0.1, -0.05]), vec![0.4, -0.2]); // gain capped at 4
        let n = normalised(&[0.2]);
        assert!((n[0] - 0.5).abs() < 1e-6); // gain 2.5
        assert!(normalised(&[]).is_empty());
    }

    #[tokio::test]
    async fn quiet_far_side_is_dropped() {
        let (e, asr) = engine("hi", "", "auto");
        assert_eq!(e.transcribe(&vec![0.001; 16000], "Them").await.unwrap(), (String::new(), true));
        assert!(asr.seen.lock().unwrap().is_empty());
        // Your own mic is never gated.
        assert_eq!(e.transcribe(&vec![0.001; 16000], "You").await.unwrap(), ("hi".into(), true));
    }

    #[tokio::test]
    async fn parakeet_gets_padded_rescored_audio_and_english_engine_short_circuits() {
        let (e, asr) = engine("bonjour tout le monde", "", "english");
        let r = e.transcribe(&vec![0.1; 48000], "You").await.unwrap();
        assert_eq!(r, ("bonjour tout le monde".into(), true));
        assert_eq!(asr.seen.lock().unwrap()[0], (48000 + TAIL_SAMPLES, true, "parakeet"));
    }

    #[tokio::test]
    async fn short_utterances_stay_english() {
        let (e, asr) = engine("xyz", "你好世界", "auto");
        e.load().await.unwrap();
        let r = e.transcribe(&vec![0.1; 16000], "You").await.unwrap();
        assert_eq!(r, ("xyz".into(), true));
        assert_eq!(asr.seen.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    #[ignore = "needs team A (looks_english)"]
    async fn sensevoice_fallback_for_non_english() {
        let (e, _) = engine("ni hao shi jie", "  你好世界 ", "auto");
        e.load().await.unwrap();
        assert_eq!(e.transcribe(&vec![0.1; 40000], "You").await.unwrap(), ("你好世界".into(), false));
        let (e, _) = engine("ni hao shi jie", "nihao", "auto");
        e.load().await.unwrap();
        assert_eq!(e.transcribe(&vec![0.1; 40000], "You").await.unwrap(), ("ni hao shi jie".into(), true));
        // Without load(), SenseVoice is never consulted.
        let (e, asr) = engine("ni hao shi jie", "你好世界", "auto");
        assert_eq!(e.transcribe(&vec![0.1; 40000], "You").await.unwrap(), ("ni hao shi jie".into(), true));
        assert_eq!(asr.seen.lock().unwrap().len(), 1);
    }
}
