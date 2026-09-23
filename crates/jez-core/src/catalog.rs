//! OWNER: team B (data). Downloadable models (ModelCatalog.swift). The downloader itself lives in jez-engines.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct CatalogModel {
    pub file: String,
    pub label: String,
    pub url: String,
    pub gb: f64,
    pub note: String,
}

/// Polish models (2B / 4B / 9B), in display order.
pub fn polish_models() -> Vec<CatalogModel> {
    todo!("team B")
}

pub fn whisper() -> CatalogModel {
    todo!("team B")
}

/// `*.gguf` in models/, sorted.
pub fn installed_models() -> Vec<String> {
    todo!("team B")
}
