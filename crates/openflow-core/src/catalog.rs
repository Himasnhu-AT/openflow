//! OWNER: team B (data). Downloadable models (ModelCatalog.swift). The downloader itself lives in openflow-engines.

use serde::Serialize;

use crate::paths;

#[derive(Debug, Clone, Serialize)]
pub struct CatalogModel {
    pub file: String,
    pub label: String,
    pub url: String,
    pub gb: f64,
    pub note: String,
}

fn model(file: &str, label: &str, url: &str, gb: f64, note: &str) -> CatalogModel {
    CatalogModel { file: file.into(), label: label.into(), url: url.into(), gb, note: note.into() }
}

/// Polish models (2B / 4B / 9B), in display order.
pub fn polish_models() -> Vec<CatalogModel> {
    vec![
        model(
            "Qwen3.5-2B-Q4_K_M.gguf",
            "Qwen3.5 2B",
            "https://huggingface.co/unsloth/Qwen3.5-2B-GGUF/resolve/main/Qwen3.5-2B-Q4_K_M.gguf",
            1.3,
            "Fastest (~0.4 s). Fine for light cleanup, weaker on self-corrections.",
        ),
        model(
            "Qwen3.5-4B-Q4_K_M.gguf",
            "Qwen3.5 4B",
            "https://huggingface.co/unsloth/Qwen3.5-4B-GGUF/resolve/main/Qwen3.5-4B-Q4_K_M.gguf",
            2.7,
            "Default. ~0.7 s per dictation, ~3 GB RAM.",
        ),
        model(
            "Qwen3.5-9B-Q4_K_M.gguf",
            "Qwen3.5 9B",
            "https://huggingface.co/unsloth/Qwen3.5-9B-GGUF/resolve/main/Qwen3.5-9B-Q4_K_M.gguf",
            5.7,
            "Most accurate. ~1.5 s per dictation, ~6 GB RAM.",
        ),
    ]
}

/// Whisper large v3 turbo for the far side of calls, run by transcribe-server (transcribe.cpp, GGUF Q8_0).
/// Revision-pinned URL and checked against `WHISPER_SHA256` after download.
pub fn whisper() -> CatalogModel {
    model(
        WHISPER_FILE,
        "Whisper large v3 turbo",
        "https://huggingface.co/handy-computer/whisper-large-v3-turbo-gguf/resolve/d222c9f621c1128299248f2ded4d8a1820519780/whisper-large-v3-turbo-Q8_0.gguf",
        0.89,
        "Far side of calls: any language, best on compressed call audio. Runs on the bundled transcribe-server.",
    )
}

pub const WHISPER_FILE: &str = "whisper-large-v3-turbo-Q8_0.gguf";
const WHISPER_SHA256: &str = "d5e65f2b0828802ae2c231673d31982cebe3a778c95d9494a9f3efee6bd17448";

/// SHA-256 a downloaded file must have, for models pinned by hash.
pub fn expected_sha256(file: &str) -> Option<&'static str> {
    (file == WHISPER_FILE).then_some(WHISPER_SHA256)
}

/// Speech models share models/ with the polish models but are not polish models.
pub fn is_speech_model(file: &str) -> bool {
    file.starts_with("whisper-") || file.starts_with("ggml-")
}

/// `*.gguf` in models/, sorted.
pub fn installed_models() -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(paths::models_dir()) else { return vec![] };
    let mut files: Vec<String> = entries
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".gguf") && !is_speech_model(n))
        .collect();
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_io::test_support::DataDir;

    #[test]
    fn catalog_entries() {
        let m = polish_models();
        assert_eq!(m.iter().map(|m| m.gb).collect::<Vec<_>>(), [1.3, 2.7, 5.7]);
        for x in m.iter().chain([whisper()].iter()) {
            assert!(x.url.ends_with(&format!("/{}", x.file)), "{}", x.url);
        }
        assert!(!whisper().url.contains("/resolve/main/"), "speech model must be revision-pinned");
        assert_eq!(expected_sha256(WHISPER_FILE).map(str::len), Some(64));
        assert_eq!(expected_sha256("Qwen3.5-4B-Q4_K_M.gguf"), None);
        assert_eq!(m[1].file, crate::Config::default().model);
        assert_eq!(whisper().file, crate::Config::default().whisper_model);
    }

    #[test]
    fn installed_lists_gguf_sorted() {
        let dir = DataDir::new();
        assert!(installed_models().is_empty());
        let models = dir.path().join("models");
        std::fs::create_dir_all(&models).unwrap();
        for f in ["b.gguf", "A.gguf", "ggml-large-v3-turbo.bin", "c.gguf.part", WHISPER_FILE] {
            std::fs::write(models.join(f), "").unwrap();
        }
        assert_eq!(installed_models(), ["A.gguf", "b.gguf"]);
    }
}

/// A model offered for `ollama pull` (the default engine). Tags and sizes from ollama.com/library/qwen3.5.
#[derive(Debug, Clone, Serialize)]
pub struct OllamaCatalogModel {
    pub name: String,
    pub label: String,
    pub gb: f64,
    pub note: String,
}

/// Recommended Ollama models, in display order.
pub fn ollama_models() -> Vec<OllamaCatalogModel> {
    [
        ("qwen3.5:2b", "Qwen3.5 2B", 2.7, "Fastest"),
        ("qwen3.5:4b", "Qwen3.5 4B", 3.4, "Default. Best balance of speed and quality"),
        ("qwen3.5:9b", "Qwen3.5 9B", 6.6, "Most accurate; good for meeting notes"),
    ]
    .into_iter()
    .map(|(name, label, gb, note)| OllamaCatalogModel { name: name.into(), label: label.into(), gb, note: note.into() })
    .collect()
}
