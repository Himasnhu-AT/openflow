//! OWNER: team A (rules). Word error rate (TextRules.wordErrorRate) for --eval.

use crate::text_rules::words;

/// Word error rate of `hypothesis` against `reference`, ignoring case and punctuation. 0 = identical words.
pub fn word_error_rate(reference: &str, hypothesis: &str) -> f64 {
    let r = words(reference);
    let h = words(hypothesis);
    if r.is_empty() {
        return if h.is_empty() { 0.0 } else { 1.0 };
    }
    let mut prev: Vec<usize> = (0..=h.len()).collect();
    for i in 1..=r.len() {
        let mut cur = vec![0; h.len() + 1];
        cur[0] = i;
        for j in 1..=h.len() {
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + usize::from(r[i - 1] != h[j - 1]));
        }
        prev = cur;
    }
    prev[h.len()] as f64 / r.len() as f64
}

#[cfg(test)]
mod tests {
    use super::word_error_rate;

    #[test]
    fn wer_edges() {
        assert_eq!(word_error_rate("", "x"), 1.0);
        assert_eq!(word_error_rate("a b", ""), 1.0);
        assert_eq!(word_error_rate("a b c d", "a x c d"), 0.25);
    }
}
