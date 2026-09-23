//! On-disk layout: `~/Library/Application Support/OpenFlow/` on macOS. File formats match the Swift app it grew
//! out of, whose `JezFlow` folder is imported on first launch (`import_legacy`).
//! `OPENFLOW_DATA_DIR` overrides the base directory (tests, side-by-side runs).

use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("OPENFLOW_DATA_DIR") {
        return PathBuf::from(d);
    }
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("OpenFlow")
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

/// Data folder of the Swift app this product grew out of (same file formats).
fn legacy_data_dir() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("JezFlow"))
}

/// First launch only: imports config, history, stats, clips and meetings from the legacy `JezFlow` folder when
/// the OpenFlow folder doesn't exist yet. Models are hard-linked (they can be several GB), copied if that fails.
/// Returns true when something was imported. Skipped when `OPENFLOW_DATA_DIR` is set.
pub fn import_legacy() -> bool {
    if std::env::var_os("OPENFLOW_DATA_DIR").is_some() {
        return false;
    }
    let (Some(from), to) = (legacy_data_dir(), data_dir()) else { return false };
    if to.exists() || !from.is_dir() {
        return false;
    }
    match copy_tree(&from, &to, false) {
        Ok(()) => {
            tracing::info!("Imported data from {}", from.display());
            true
        }
        Err(e) => {
            tracing::error!("Import from {} failed: {e}", from.display());
            false
        }
    }
}

fn copy_tree(from: &std::path::Path, to: &std::path::Path, link: bool) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        if entry.file_type()?.is_dir() {
            copy_tree(&src, &dst, link || entry.file_name() == "models")?;
        } else if !(link && std::fs::hard_link(&src, &dst).is_ok()) {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn copy_tree_links_models_and_copies_the_rest() {
        let base = std::env::temp_dir().join(format!("openflow-import-{}", std::process::id()));
        let (from, to) = (base.join("old"), base.join("new"));
        std::fs::create_dir_all(from.join("models")).unwrap();
        std::fs::create_dir_all(from.join("meetings")).unwrap();
        std::fs::write(from.join("config.json"), "{}").unwrap();
        std::fs::write(from.join("meetings/A.json"), "{}").unwrap();
        std::fs::write(from.join("models/m.gguf"), "weights").unwrap();
        super::copy_tree(&from, &to, false).unwrap();
        assert_eq!(std::fs::read_to_string(to.join("config.json")).unwrap(), "{}");
        assert!(to.join("meetings/A.json").exists());
        assert_eq!(std::fs::read_to_string(to.join("models/m.gguf")).unwrap(), "weights");
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(std::fs::metadata(to.join("models/m.gguf")).unwrap().ino(), std::fs::metadata(from.join("models/m.gguf")).unwrap().ino());
        }
        let _ = std::fs::remove_dir_all(&base);
    }
}
