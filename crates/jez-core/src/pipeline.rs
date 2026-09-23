//! OWNER: team A (rules). Mirrors Pipeline.swift: raw transcript -> text that gets pasted.

use crate::traits::{Examples, PolishBackend};
use crate::{Config, FieldContext};

pub fn has_correction_signal(_text: &str, _learned: &[String]) -> bool {
    todo!("team A")
}

/// Shared by the app and `--eval` so a replay scores exactly what a real dictation would have produced.
pub async fn run(
    _raw: &str,
    _tone: &str,
    _config: &Config,
    _polisher: Option<&dyn PolishBackend>,
    _examples: &Examples,
    _context: Option<&FieldContext>,
    _on_partial: Option<&(dyn Fn(&str) + Send + Sync)>,
) -> anyhow::Result<String> {
    todo!("team A")
}
