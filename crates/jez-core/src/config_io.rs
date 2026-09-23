//! OWNER: team B (data). Load/save of config.json (see Config.swift / Store.swift init).
use std::fs;

use crate::config::Config;
use crate::{paths, swift_json};

/// Config.swift `load()` plus the Store.swift init fallback: any failure (unreadable or invalid file) keeps the
/// broken file as `config.json.bak` and returns the defaults, so the next save doesn't lose the user's edits.
pub(crate) fn load() -> anyhow::Result<Config> {
    match try_load() {
        Ok(c) => Ok(c),
        Err(e) => {
            let file = paths::config_file();
            let backup = file.with_extension("json.bak");
            let _ = fs::remove_file(&backup);
            let _ = fs::copy(&file, &backup);
            tracing::error!(target: "store", "Config unreadable (backed up to config.json.bak): {e:#}");
            Ok(Config::default())
        }
    }
}

fn try_load() -> anyhow::Result<Config> {
    fs::create_dir_all(paths::models_dir())?;
    let file = paths::config_file();
    if !file.exists() {
        save(&Config::default())?;
    }
    parse(&fs::read(&file)?)
}

/// Decodes config.json like Swift's `init(from:)`: missing or `null` keys take their defaults.
pub fn parse(bytes: &[u8]) -> anyhow::Result<Config> {
    let mut v: serde_json::Value = serde_json::from_slice(bytes)?;
    if !v.is_object() {
        anyhow::bail!("config.json is not a JSON object");
    }
    swift_json::strip_top_level_nulls(&mut v);
    Ok(serde_json::from_value(v)?)
}

/// `JSONEncoder` `[.prettyPrinted, .sortedKeys]` text of the config.
pub fn to_json(config: &Config) -> anyhow::Result<String> {
    swift_json::to_pretty(config)
}

pub(crate) fn save(config: &Config) -> anyhow::Result<()> {
    let file = paths::config_file();
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(file, to_json(config)?)?;
    Ok(())
}

/// Tests that touch the data dir point `JEZFLOW_DATA_DIR` at a fresh temp dir, one test at a time.
#[cfg(test)]
pub(crate) mod test_support {
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, MutexGuard};

    static LOCK: Mutex<()> = Mutex::new(());

    pub struct DataDir {
        path: PathBuf,
        _guard: MutexGuard<'static, ()>,
    }

    impl DataDir {
        #[allow(clippy::new_without_default)]
        pub fn new() -> Self {
            let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let path = std::env::temp_dir().join(format!("jezflow-test-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).unwrap();
            std::env::set_var("JEZFLOW_DATA_DIR", &path);
            DataDir { path, _guard: guard }
        }
        pub fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for DataDir {
        fn drop(&mut self) {
            std::env::remove_var("JEZFLOW_DATA_DIR");
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::DataDir;
    use super::*;

    #[test]
    fn default_config_matches_swift_bytes() {
        assert_eq!(to_json(&Config::default()).unwrap(), include_str!("../testdata/swift/config_default.json"));
    }

    #[test]
    fn odd_config_round_trips_byte_exact() {
        let swift = include_str!("../testdata/swift/config_odd.json");
        let c = parse(swift.as_bytes()).unwrap();
        assert!((c.tap_threshold - 0.30000000000000004).abs() < 1e-18);
        assert!(c.memory.contains("</script>") && c.memory.contains('\u{1}'));
        assert_eq!(c.tones.get("empty").map(String::as_str), Some(""));
        assert_eq!(to_json(&c).unwrap(), swift);
        let empty = include_str!("../testdata/swift/config_empty.json");
        let c = parse(empty.as_bytes()).unwrap();
        assert!(c.app_tones.is_empty() && c.dictionary.is_empty());
        assert_eq!(to_json(&c).unwrap(), empty);
    }

    #[test]
    fn missing_null_and_legacy_keys() {
        let c = parse(br#"{"model": "x.gguf", "memory": null, "cloudDictation": true, "somethingNew": 1}"#).unwrap();
        let d = Config::default();
        assert_eq!(c.model, "x.gguf");
        assert_eq!(c.memory, "");
        assert_eq!(c.cloud_dictation, "all");
        assert_eq!(c.tones, d.tones);
        assert_eq!(c.live_text_excluded, d.live_text_excluded);
        assert_eq!(parse(br#"{"cloudDictation": false}"#).unwrap().cloud_dictation, "off");
        assert_eq!(parse(br#"{"cloudDictation": "long"}"#).unwrap().cloud_dictation, "long");
        assert_eq!(parse(b"{}").unwrap(), d);
        assert!(parse(br#"{"llamaPort": "x"}"#).is_err());
        assert!(parse(b"[]").is_err());
    }

    #[test]
    fn load_writes_defaults_then_backs_up_broken_file() {
        let dir = DataDir::new();
        let c = Config::load().unwrap();
        assert_eq!(c, Config::default());
        assert!(dir.path().join("models").is_dir());
        let written = fs::read_to_string(dir.path().join("config.json")).unwrap();
        assert_eq!(written, include_str!("../testdata/swift/config_default.json"));

        let mut c2 = c.clone();
        c2.memory = "I am Jez".into();
        c2.save().unwrap();
        assert_eq!(Config::load().unwrap().memory, "I am Jez");

        fs::write(dir.path().join("config.json"), "{ not json").unwrap();
        assert_eq!(Config::load().unwrap(), Config::default());
        assert_eq!(fs::read_to_string(dir.path().join("config.json.bak")).unwrap(), "{ not json");
        // The broken file itself is left alone until the next save.
        assert_eq!(fs::read_to_string(dir.path().join("config.json")).unwrap(), "{ not json");
    }
}
