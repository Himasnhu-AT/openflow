//! OWNER: team C (engines). Meeting notes map-reduce + title (Polisher.summarizeMeeting / meetingTitle).

use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;

use crate::prompts::{chunk_system, merge_system, prefix_chars, TITLE_SYSTEM};

/// Words per chunk: an hour-long meeting has to fit the 4k context.
pub const CHUNK_WORDS: usize = 1300;
/// Notes merged per merge call.
pub const MERGE_GROUP: usize = 4;

/// One system+user completion (implemented by `Polisher`; tests use a fake).
#[async_trait]
pub trait Completer: Send + Sync {
    async fn complete(&self, system: &str, user: &str, max_tokens: usize, timeout: Duration) -> Result<String>;
}

/// Chunked map-reduce: notes per ~1300 words, then merged four at a time until one set is left.
/// `memory` is `config.memory`.
pub async fn summarize_meeting<C: Completer + ?Sized>(c: &C, transcript: &str, memory: &str) -> Result<String> {
    // Swift `split(separator: " ")` drops empty pieces; newlines stay inside words.
    let words: Vec<&str> = transcript.split(' ').filter(|w| !w.is_empty()).collect();
    let chunks: Vec<String> = words.chunks(CHUNK_WORDS).map(|w| w.join(" ")).collect();
    if chunks.is_empty() {
        return Ok(String::new());
    }
    let system = chunk_system(memory);
    let mut notes = Vec::with_capacity(chunks.len());
    for chunk in &chunks {
        notes.push(c.complete(&system, chunk, 700, Duration::from_secs(180)).await?);
    }
    while notes.len() > 1 {
        let system = merge_system();
        let mut merged = Vec::with_capacity(notes.len().div_ceil(MERGE_GROUP));
        for group in notes.chunks(MERGE_GROUP) {
            merged.push(c.complete(&system, &group.join("\n\n---\n\n"), 900, Duration::from_secs(180)).await?);
        }
        notes = merged;
    }
    Ok(notes.swap_remove(0))
}

/// A short title for a meeting from its notes.
pub async fn meeting_title<C: Completer + ?Sized>(c: &C, summary: &str) -> Result<String> {
    let t = c.complete(TITLE_SYSTEM, summary, 24, Duration::from_secs(60)).await?;
    let t = t.trim_matches(|ch| matches!(ch, '"' | '\'' | '.' | ' ' | '\n'));
    Ok(prefix_chars(t, 80).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Records calls; replies "N<i>" (or a fixed reply).
    #[derive(Default)]
    struct Fake {
        calls: Mutex<Vec<(String, String, usize, Duration)>>,
        reply: Option<String>,
    }

    #[async_trait]
    impl Completer for Fake {
        async fn complete(&self, system: &str, user: &str, max_tokens: usize, timeout: Duration) -> Result<String> {
            let mut calls = self.calls.lock().unwrap();
            calls.push((system.into(), user.into(), max_tokens, timeout));
            Ok(self.reply.clone().unwrap_or_else(|| format!("N{}", calls.len())))
        }
    }

    fn words(n: usize) -> String {
        (0..n).map(|i| format!("w{i}")).collect::<Vec<_>>().join(" ")
    }

    #[tokio::test]
    async fn empty_transcript_makes_no_calls() {
        let f = Fake::default();
        assert_eq!(summarize_meeting(&f, "   ", "").await.unwrap(), "");
        assert!(f.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn single_chunk_no_merge() {
        let f = Fake::default();
        let out = summarize_meeting(&f, "  hello   there\nfriend ", "").await.unwrap();
        assert_eq!(out, "N1");
        let calls = f.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, "hello there\nfriend");
        assert_eq!(calls[0].2, 700);
        assert_eq!(calls[0].3, Duration::from_secs(180));
        assert!(calls[0].0.starts_with("You take notes for the person"));
    }

    #[tokio::test]
    async fn chunks_and_merges_in_groups_of_four() {
        // 6 chunks (last one partial) -> merge [N1..N4], [N5,N6] -> merge [N7, N8] -> N9.
        let f = Fake::default();
        let out = summarize_meeting(&f, &words(CHUNK_WORDS * 5 + 7), "I'm Taylor").await.unwrap();
        assert_eq!(out, "N9");
        let calls = f.calls.lock().unwrap();
        assert_eq!(calls.len(), 9);
        assert_eq!(calls[0].1.split(' ').count(), CHUNK_WORDS);
        assert_eq!(calls[5].1.split(' ').count(), 7);
        assert!(calls[0].0.contains("About that person: I'm Taylor When the call"));
        assert_eq!(calls[6].1, "N1\n\n---\n\nN2\n\n---\n\nN3\n\n---\n\nN4");
        assert_eq!(calls[7].1, "N5\n\n---\n\nN6");
        assert_eq!(calls[8].1, "N7\n\n---\n\nN8");
        for c in &calls[6..] {
            assert_eq!(c.2, 900);
            assert!(c.0.starts_with("Below are notes from consecutive parts"));
        }
    }

    #[tokio::test]
    async fn title_trimmed_and_capped() {
        let f = Fake { reply: Some("\"Pricing page launch and reel.\"\n".into()), ..Default::default() };
        assert_eq!(meeting_title(&f, "notes").await.unwrap(), "Pricing page launch and reel");
        let first = f.calls.lock().unwrap()[0].clone();
        assert_eq!((first.2, first.3), (24, Duration::from_secs(60)));
        let f = Fake { reply: Some("x".repeat(100)), ..Default::default() };
        assert_eq!(meeting_title(&f, "n").await.unwrap().len(), 80);
    }
}
