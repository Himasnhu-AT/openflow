//! OWNER: team E (platform). Live text in the field + final swap (LiveTyper.swift), written against Keyboard +
//! FieldReader so it is cross-platform.
//!
//! Shows the live transcript in the focused field while you talk, then removes it so the polished text can be
//! pasted. Text is inserted by pasting, never by keystrokes: apps autocorrect what is typed (Notes fixes
//! spelling, capitalises, swaps smart punctuation), which made backspace counts drift and eat surrounding text.
//! Pasted text is left alone, so the count of what we inserted stays exact. The field's cursor position is not
//! used for counting either; it proved unreliable mid-edit in Notes.
//!
//! Calls must be serialized (the Swift type was `@MainActor`): hold it behind a `tokio::sync::Mutex` or own it
//! from a single task.

use std::sync::Arc;
use std::time::Duration;

use openflow_core::traits::{FieldReader, Keyboard};
use openflow_core::FieldContext;
use tokio::time::{sleep, Instant};

use crate::text16::{char_count, common_prefix_chars, drop_chars, prefix_chars, utf16_len};

/// Live updates only rewrite this many trailing characters; earlier words stay as first shown (the final swap
/// fixes everything anyway).
const TAIL_WINDOW: usize = 80;
/// Backspace virtual key code (macOS); the Keyboard trait hides the code, kept here for documentation.
#[allow(dead_code)]
const BACKSPACE: u16 = 51;

pub struct LiveTyper {
    keyboard: Arc<dyn Keyboard>,
    field: Arc<dyn FieldReader>,
    typed: String,
    saved_clipboard: Option<String>,
    /// Text before the cursor at begin(), when the field could be read; used to check that the final paste landed.
    field_before: Option<String>,
    cursor: Option<usize>,
    desynced: bool,
}

/// The live text's real extent in the field.
struct Measured {
    start: usize,
    length: usize,
    text: String,
}

impl LiveTyper {
    pub fn new(keyboard: Arc<dyn Keyboard>, field: Arc<dyn FieldReader>) -> Self {
        LiveTyper {
            keyboard,
            field,
            typed: String::new(),
            saved_clipboard: None,
            field_before: None,
            cursor: None,
            desynced: false,
        }
    }

    /// What we believe is currently inserted in the field.
    pub fn typed(&self) -> &str {
        &self.typed
    }

    /// True once live typing stopped for this dictation because a paste never landed.
    pub fn is_desynced(&self) -> bool {
        self.desynced
    }

    pub fn begin(&mut self, context: Option<&FieldContext>) {
        self.typed.clear();
        self.saved_clipboard = self.keyboard.clipboard_get();
        self.field_before = context.and_then(|c| {
            if c.before.is_empty() && c.window_title.is_empty() {
                None
            } else {
                Some(c.before.clone())
            }
        });
        self.cursor = context.and_then(|c| c.cursor);
        self.desynced = false;
    }

    /// Live hypothesis: trailing punctuation is dropped so a "." that flickers on and off doesn't cause churn.
    pub async fn update(&mut self, text: &str) {
        if self.desynced {
            return;
        }
        let replaced = text.replace('\n', " ");
        let next = replaced.trim_end_matches(['.', ',', ' ']);
        if next == self.typed {
            return;
        }
        let typed_count = char_count(&self.typed);
        let common = common_prefix_chars(&self.typed, next);
        let keep = common.max(typed_count.saturating_sub(TAIL_WINDOW)).min(char_count(next));
        self.backspace(typed_count - keep);
        let added = drop_chars(next, keep).to_string();
        self.typed = format!("{}{}", prefix_chars(&self.typed, keep), added);
        if self.insert(&added) {
            // The paste is delivered asynchronously; the clipboard must not change until the app has consumed it.
            // Where the field can be read, wait until its cursor shows the paste landed; if it never does, stop
            // typing live (the final swap measures what is really there).
            if let Some(start) = self.cursor {
                let expected = start + utf16_len(&self.typed);
                let mut landed = false;
                for _ in 0..6 {
                    sleep(Duration::from_millis(100)).await;
                    if self.field.capture().cursor.is_some_and(|c| c >= expected) {
                        landed = true;
                        break;
                    }
                }
                if !landed {
                    self.desynced = true;
                    tracing::error!(target: "typer", "Live text out of sync with the field; stopping live typing for this dictation");
                }
            } else {
                sleep(Duration::from_millis(120)).await;
            }
        }
    }

    /// The live text's real extent in the field: from where the cursor was at begin() to where it is now.
    fn measured_live(&self) -> Option<Measured> {
        let start = self.cursor?;
        let now = self.field.capture();
        let cur = now.cursor?;
        if cur < start {
            return None;
        }
        let typed16 = utf16_len(&self.typed);
        let slack = 8.max(typed16 / 5);
        let length = cur - start;
        if length > typed16 + slack {
            return None;
        }
        let before: Vec<u16> = now.before.encode_utf16().collect();
        if before.len() < length {
            return None;
        }
        let text = String::from_utf16_lossy(&before[before.len() - length..]);
        Some(Measured { start, length, text })
    }

    /// Removes exactly what was inserted (cancel, or nothing to paste).
    pub async fn retract(&mut self) {
        let n = self.measured_live().map(|m| char_count(&m.text)).unwrap_or_else(|| char_count(&self.typed));
        tracing::info!(target: "typer", "Retract {n} chars");
        self.backspace(n);
        self.typed.clear();
        sleep(Duration::from_secs(1)).await;
        self.restore_clipboard();
    }

    /// Replaces the live text with the final text in one go: backspaces, one paste, then the user's clipboard is
    /// put back once the paste has been consumed. A single ordered sequence, so nothing can race the paste.
    pub async fn finish(&mut self, text: &str) {
        let mut to_remove = char_count(&self.typed);
        // Where the field can be read, the live text's real extent is measured (from the cursor position at start
        // to the cursor now) instead of trusting what we think we inserted: pastes can land late or get dropped.
        if let Some(m) = self.measured_live() {
            to_remove = char_count(&m.text);
            // Apple's text views can swap the measured range for the final text in one operation: instant, exact.
            if self.field.replace(m.start, m.length, &m.text, text) {
                tracing::info!(target: "typer", "Finish: replaced {} units in place with {}", m.length, char_count(text));
                self.typed.clear();
                self.restore_clipboard();
                return;
            }
        } else if self.desynced {
            tracing::error!(target: "typer", "Finish: field unreadable after a desync; leaving the live text and appending the final text");
            to_remove = 0;
        }
        tracing::info!(target: "typer",
            "Finish: retract {to_remove} chars (typed {}), paste {}", char_count(&self.typed), char_count(text));
        self.backspace(to_remove);
        self.typed.clear();
        self.insert(text);
        // The paste is delivered asynchronously, behind every backspace above; a slow app can take seconds to get
        // through them. Restoring the clipboard before the paste lands pastes the old clipboard instead. So: wait
        // until the field shows the pasted text where it can be read, otherwise wait in proportion to the queue.
        let probe = prefix_chars(text.trim(), 30).to_string();
        let deadline = Instant::now() + Duration::from_secs_f64(2.0 + to_remove as f64 * 0.03);
        let mut landed = false;
        if self.field_before.is_some() && char_count(&probe) >= 6 {
            while Instant::now() < deadline {
                sleep(Duration::from_millis(250)).await;
                if self.field.capture().before.contains(&probe) {
                    landed = true;
                    break;
                }
            }
        }
        if !landed {
            let remaining = deadline.saturating_duration_since(Instant::now());
            sleep(remaining + Duration::from_millis(1500)).await;
        }
        self.restore_clipboard();
    }

    /// The inserted text is the final result: stop tracking it.
    pub fn forget(&mut self) {
        self.typed.clear();
        self.restore_clipboard();
    }

    fn restore_clipboard(&mut self) {
        if let Some(saved) = self.saved_clipboard.take() {
            self.keyboard.clipboard_set(&saved);
        }
    }

    fn backspace(&self, n: usize) {
        if n > 0 {
            self.keyboard.backspace(n);
        }
    }

    fn insert(&self, s: &str) -> bool {
        if s.is_empty() {
            return false;
        }
        self.keyboard.clipboard_set(s);
        self.keyboard.paste_shortcut();
        true
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::text16::context_from_value;
    use openflow_core::model::AppInfo;
    use std::sync::Mutex;
    use unicode_segmentation::UnicodeSegmentation;

    /// A text field plus the keyboard/clipboard that edits it, all simulated.
    #[derive(Default)]
    pub struct Sim {
        pub text: String,
        /// Cursor in UTF-16 units.
        pub cursor: usize,
        pub clipboard: Option<String>,
        pub title: String,
        /// Field reports a selection range (cursor).
        pub cursor_readable: bool,
        /// Field value readable at all.
        pub readable: bool,
        /// Pastes are dropped by the app.
        pub drop_pastes: bool,
        /// Each paste lands only after this many `capture()` calls.
        pub paste_delay: usize,
        pending: Vec<(String, usize)>,
        /// FieldReader::replace works (Apple text view).
        pub supports_replace: bool,
        pub replace_calls: usize,
        pub pastes: Vec<String>,
        pub backspaces: usize,
        pub clipboard_sets: Vec<String>,
    }

    impl Sim {
        fn apply_paste(&mut self, s: &str) {
            let idx = byte_index(&self.text, self.cursor);
            self.text.insert_str(idx, s);
            self.cursor += utf16_len(s);
        }
        fn tick_pending(&mut self) {
            let mut still = Vec::new();
            for (s, n) in std::mem::take(&mut self.pending) {
                if n <= 1 {
                    self.apply_paste(&s);
                } else {
                    still.push((s, n - 1));
                }
            }
            self.pending = still;
        }
    }

    fn byte_index(s: &str, utf16: usize) -> usize {
        let mut u = 0;
        for (i, c) in s.char_indices() {
            if u >= utf16 {
                return i;
            }
            u += c.len_utf16();
        }
        s.len()
    }

    #[derive(Clone)]
    pub struct Fake(pub Arc<Mutex<Sim>>);

    impl Fake {
        pub fn new(text: &str, cursor_at: Option<usize>) -> Fake {
            let sim = Sim {
                text: text.to_string(),
                cursor: cursor_at.unwrap_or(utf16_len(text)),
                cursor_readable: true,
                readable: true,
                title: "Doc".into(),
                ..Default::default()
            };
            Fake(Arc::new(Mutex::new(sim)))
        }
        pub fn sim(&self) -> std::sync::MutexGuard<'_, Sim> {
            self.0.lock().unwrap()
        }
        fn typer(&self) -> LiveTyper {
            LiveTyper::new(Arc::new(self.clone()), Arc::new(self.clone()))
        }
    }

    impl Keyboard for Fake {
        fn paste_shortcut(&self) {
            let mut s = self.sim();
            let clip = s.clipboard.clone().unwrap_or_default();
            s.pastes.push(clip.clone());
            if s.drop_pastes {
                return;
            }
            if s.paste_delay > 0 {
                let d = s.paste_delay;
                s.pending.push((clip, d));
            } else {
                s.apply_paste(&clip);
            }
        }
        fn backspace(&self, count: usize) {
            let mut s = self.sim();
            s.backspaces += count;
            for _ in 0..count {
                let idx = byte_index(&s.text, s.cursor);
                let Some((start, g)) = s.text[..idx].grapheme_indices(true).next_back() else { break };
                let len16 = utf16_len(g);
                s.text.replace_range(start..idx, "");
                s.cursor -= len16;
            }
        }
        fn clipboard_get(&self) -> Option<String> {
            self.sim().clipboard.clone()
        }
        fn clipboard_set(&self, text: &str) {
            let mut s = self.sim();
            s.clipboard = Some(text.to_string());
            s.clipboard_sets.push(text.to_string());
        }
    }

    impl FieldReader for Fake {
        fn capture(&self) -> FieldContext {
            let mut s = self.sim();
            s.tick_pending();
            if !s.readable {
                return FieldContext { window_title: s.title.clone(), ..Default::default() };
            }
            let loc = s.cursor_readable.then_some(s.cursor as isize);
            context_from_value(s.title.clone(), &s.text.clone(), loc)
        }
        fn replace(&self, location: usize, length: usize, _expecting: &str, text: &str) -> bool {
            let mut s = self.sim();
            s.replace_calls += 1;
            if !s.supports_replace {
                return false;
            }
            let a = byte_index(&s.text, location);
            let b = byte_index(&s.text, location + length);
            s.text.replace_range(a..b, text);
            s.cursor = location + utf16_len(text);
            true
        }
        fn frontmost_app(&self) -> Option<AppInfo> {
            None
        }
        fn is_trusted(&self, _prompt: bool) -> bool {
            true
        }
    }

    fn with_clip(f: &Fake, clip: &str) {
        f.sim().clipboard = Some(clip.into());
    }

    #[tokio::test(start_paused = true)]
    async fn live_updates_then_finish_by_keystrokes() {
        let f = Fake::new("Hi ", None);
        with_clip(&f, "user clip");
        let mut t = f.typer();
        let ctx = f.capture();
        t.begin(Some(&ctx));
        t.update("hello").await;
        assert_eq!(f.sim().text, "Hi hello");
        t.update("hello world.").await; // trailing "." dropped
        assert_eq!(f.sim().text, "Hi hello world");
        assert_eq!(t.typed(), "hello world");
        t.update("hello word").await; // common prefix "hello wor", rewrite tail
        assert_eq!(f.sim().text, "Hi hello word");
        assert_eq!(f.sim().pastes.last().unwrap(), "d");
        t.finish("Hello, world!").await;
        assert_eq!(f.sim().text, "Hi Hello, world!");
        assert_eq!(f.sim().clipboard.as_deref(), Some("user clip"));
        assert!(!t.is_desynced());
    }

    #[tokio::test(start_paused = true)]
    async fn unchanged_update_is_noop() {
        let f = Fake::new("", None);
        let mut t = f.typer();
        t.begin(Some(&f.capture()));
        t.update("abc").await;
        let n = f.sim().pastes.len();
        t.update("abc, ").await;
        t.update("abc\n").await;
        assert_eq!(f.sim().pastes.len(), n);
    }

    #[tokio::test(start_paused = true)]
    async fn newline_becomes_space() {
        let f = Fake::new("", None);
        let mut t = f.typer();
        t.begin(Some(&f.capture()));
        t.update("a\nb").await;
        assert_eq!(f.sim().text, "a b");
    }

    #[tokio::test(start_paused = true)]
    async fn tail_window_limits_rewrites() {
        let f = Fake::new("", None);
        let mut t = f.typer();
        t.begin(Some(&f.capture()));
        let first = "a".repeat(100);
        t.update(&first).await;
        // Change at position 0: only the last 80 chars are rewritten; the first 20 stay as first shown.
        let second = format!("b{}", "a".repeat(99));
        t.update(&second).await;
        assert_eq!(f.sim().backspaces, 80);
        assert_eq!(t.typed(), format!("{}{}", "a".repeat(20), &second[20..]));
        assert_eq!(f.sim().text, t.typed());
    }

    #[tokio::test(start_paused = true)]
    async fn shrinking_hypothesis() {
        let f = Fake::new("", None);
        let mut t = f.typer();
        t.begin(Some(&f.capture()));
        t.update("one two three").await;
        t.update("one two").await;
        assert_eq!(f.sim().text, "one two");
        assert_eq!(t.typed(), "one two");
    }

    #[tokio::test(start_paused = true)]
    async fn emoji_counts_as_one_backspace() {
        let f = Fake::new("", None);
        let mut t = f.typer();
        t.begin(Some(&f.capture()));
        t.update("hi 👍🏽 x").await;
        t.update("hi 👍🏽 y").await;
        assert_eq!(f.sim().text, "hi 👍🏽 y");
        t.update("hi").await;
        assert_eq!(f.sim().text, "hi");
    }

    #[tokio::test(start_paused = true)]
    async fn late_paste_within_polling_is_fine() {
        let f = Fake::new("x", None);
        f.sim().paste_delay = 3;
        let mut t = f.typer();
        t.begin(Some(&f.capture()));
        t.update("hello").await;
        assert!(!t.is_desynced());
        assert_eq!(f.sim().text, "xhello");
    }

    #[tokio::test(start_paused = true)]
    async fn dropped_paste_desyncs_and_finish_appends() {
        let f = Fake::new("start ", None);
        f.sim().drop_pastes = true;
        let mut t = f.typer();
        t.begin(Some(&f.capture()));
        let t0 = Instant::now();
        t.update("hello").await;
        assert!(t.is_desynced());
        assert_eq!(Instant::now() - t0, Duration::from_millis(600)); // 6 × 100 ms
        let pastes = f.sim().pastes.len();
        t.update("hello world").await; // ignored after desync
        assert_eq!(f.sim().pastes.len(), pastes);
        f.sim().drop_pastes = false;
        // measuredLive works (cursor unchanged => length 0), so it removes 0 and pastes the final text.
        t.finish("Final.").await;
        assert_eq!(f.sim().text, "start Final.");
        assert_eq!(f.sim().backspaces, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn desync_with_unreadable_field_appends_without_backspace() {
        let f = Fake::new("abc", None);
        let mut t = f.typer();
        t.begin(Some(&f.capture()));
        f.sim().paste_delay = 100; // never lands during polling
        t.update("hello").await;
        assert!(t.is_desynced());
        f.sim().cursor_readable = false;
        f.sim().paste_delay = 0;
        t.finish("Done").await;
        assert_eq!(f.sim().backspaces, 0);
        assert!(f.sim().text.ends_with("Done"));
    }

    #[tokio::test(start_paused = true)]
    async fn no_cursor_uses_typed_count_and_fixed_wait() {
        let f = Fake::new("pre ", None);
        f.sim().cursor_readable = false;
        let mut t = f.typer();
        let ctx = f.capture();
        assert_eq!(ctx.cursor, None);
        t.begin(Some(&ctx));
        let t0 = Instant::now();
        t.update("abc").await;
        assert_eq!(Instant::now() - t0, Duration::from_millis(120));
        t.update("abd").await;
        assert_eq!(f.sim().text, "pre abd");
        t.finish("ABD!").await;
        assert_eq!(f.sim().text, "pre ABD!");
    }

    #[tokio::test(start_paused = true)]
    async fn finish_uses_replace_fast_path() {
        let f = Fake::new("Dear Bob, ", None);
        f.sim().supports_replace = true;
        with_clip(&f, "clip");
        let mut t = f.typer();
        t.begin(Some(&f.capture()));
        t.update("thanks for the").await;
        let backspaces = f.sim().backspaces;
        let t0 = Instant::now();
        t.finish("Thanks for the update.").await;
        assert_eq!(Instant::now(), t0); // no waiting
        assert_eq!(f.sim().text, "Dear Bob, Thanks for the update.");
        assert_eq!(f.sim().backspaces, backspaces);
        assert_eq!(f.sim().replace_calls, 1);
        assert_eq!(f.sim().clipboard.as_deref(), Some("clip"));
    }

    #[tokio::test(start_paused = true)]
    async fn finish_measures_real_extent() {
        // The app dropped the second paste, so the field holds less than we think; measuredLive removes only
        // what is really there instead of eating surrounding text.
        let f = Fake::new("Keep this. ", None);
        let mut t = f.typer();
        t.begin(Some(&f.capture()));
        t.update("one").await;
        f.sim().paste_delay = 100;
        t.update("one two").await; // " two" never lands -> desync
        assert!(t.is_desynced());
        f.sim().pending.clear();
        f.sim().paste_delay = 0;
        t.finish("One two.").await;
        assert_eq!(f.sim().text, "Keep this. One two.");
    }

    #[tokio::test(start_paused = true)]
    async fn clipboard_restore_waits_for_probe() {
        let f = Fake::new("x ", None);
        with_clip(&f, "orig");
        f.sim().paste_delay = 2; // final paste lands after 2 captures
        let mut t = f.typer();
        t.begin(Some(&f.capture()));
        let t0 = Instant::now();
        t.finish("This is the final text").await;
        // Polls every 250 ms: the paste lands on the second poll.
        assert_eq!(Instant::now() - t0, Duration::from_millis(500));
        assert_eq!(f.sim().clipboard.as_deref(), Some("orig"));
        assert_eq!(f.sim().text, "x This is the final text");
    }

    #[tokio::test(start_paused = true)]
    async fn clipboard_restore_deadline_when_unconfirmed() {
        let f = Fake::new("", None);
        f.sim().readable = false;
        with_clip(&f, "orig");
        let mut t = f.typer();
        t.begin(None);
        t.update("abc").await;
        let t0 = Instant::now();
        t.finish("final text here").await;
        // 3 backspaces: 2.0 + 0.09 s deadline + 1.5 s.
        assert_eq!(Instant::now() - t0, Duration::from_millis(3590));
        assert_eq!(f.sim().clipboard.as_deref(), Some("orig"));
    }

    #[tokio::test(start_paused = true)]
    async fn short_probe_skips_polling() {
        let f = Fake::new("", None);
        with_clip(&f, "orig");
        let mut t = f.typer();
        t.begin(Some(&f.capture()));
        let t0 = Instant::now();
        t.finish("Ok.").await;
        assert_eq!(Instant::now() - t0, Duration::from_millis(3500));
    }

    #[tokio::test(start_paused = true)]
    async fn retract_removes_live_text() {
        let f = Fake::new("Before ", None);
        with_clip(&f, "orig");
        let mut t = f.typer();
        t.begin(Some(&f.capture()));
        t.update("some words").await;
        let t0 = Instant::now();
        t.retract().await;
        assert_eq!(Instant::now() - t0, Duration::from_secs(1));
        assert_eq!(f.sim().text, "Before ");
        assert_eq!(t.typed(), "");
        assert_eq!(f.sim().clipboard.as_deref(), Some("orig"));
    }

    #[tokio::test(start_paused = true)]
    async fn retract_without_cursor_uses_typed() {
        let f = Fake::new("Before ", None);
        f.sim().cursor_readable = false;
        let mut t = f.typer();
        t.begin(Some(&f.capture()));
        t.update("abc").await;
        t.retract().await;
        assert_eq!(f.sim().text, "Before ");
    }

    #[tokio::test(start_paused = true)]
    async fn measured_live_rejects_implausible_extent() {
        // The user clicked elsewhere; cursor moved far beyond the live text: fall back to typed count.
        let f = Fake::new("", None);
        let mut t = f.typer();
        t.begin(Some(&f.capture()));
        t.update("abc").await;
        {
            let mut s = f.sim();
            s.text.push_str(&"z".repeat(50));
            s.cursor = utf16_len(&s.text);
        }
        t.retract().await;
        assert_eq!(f.sim().text, format!("abc{}", "z".repeat(47)));
    }

    #[tokio::test(start_paused = true)]
    async fn forget_restores_clipboard_once() {
        let f = Fake::new("", None);
        with_clip(&f, "orig");
        let mut t = f.typer();
        t.begin(Some(&f.capture()));
        t.update("abc").await;
        t.forget();
        assert_eq!(f.sim().clipboard.as_deref(), Some("orig"));
        assert_eq!(f.sim().text, "abc");
        let sets = f.sim().clipboard_sets.len();
        t.forget();
        assert_eq!(f.sim().clipboard_sets.len(), sets);
    }

    #[tokio::test(start_paused = true)]
    async fn no_saved_clipboard_is_not_restored() {
        let f = Fake::new("", None);
        let mut t = f.typer();
        t.begin(Some(&f.capture()));
        t.update("abc").await;
        t.forget();
        assert_eq!(f.sim().clipboard.as_deref(), Some("abc"));
    }

    #[tokio::test(start_paused = true)]
    async fn utf16_cursor_math_with_astral_chars() {
        let f = Fake::new("𝒳 ", None);
        let mut t = f.typer();
        t.begin(Some(&f.capture()));
        t.update("𝒜 b").await;
        assert!(!t.is_desynced());
        t.update("𝒜 c").await;
        assert!(!t.is_desynced());
        t.finish("𝒜 C done").await;
        assert_eq!(f.sim().text, "𝒳 𝒜 C done");
    }
}
