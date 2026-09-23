//! On-disk layout, shared with the Swift app: `~/Library/Application Support/JezFlow/` on macOS.
//! `JEZFLOW_DATA_DIR` overrides the base directory (tests, side-by-side runs).

use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("JEZFLOW_DATA_DIR") {
        return PathBuf::from(d);
    }
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("JezFlow")
}

pub fn config_file() -> PathBuf {
    data_dir().join("config.json")
}
pub fn models_dir() -> PathBuf {
    data_dir().join("models")
}
pub fn history_file() -> PathBuf {
    data_dir().join("history.json")
}
pub fn stats_file() -> PathBuf {
    data_dir().join("stats.json")
}
pub fn clips_dir() -> PathBuf {
    data_dir().join("clips")
}
pub fn meetings_dir() -> PathBuf {
    data_dir().join("meetings")
}
