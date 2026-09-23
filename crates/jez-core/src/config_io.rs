//! OWNER: team B (data). Load/save of config.json (see Config.swift / Store.swift init).
use crate::config::Config;

pub(crate) fn load() -> anyhow::Result<Config> {
    todo!("team B: create models dir, write defaults if missing, parse; on parse failure copy to config.json.bak and return defaults")
}

pub(crate) fn save(_config: &Config) -> anyhow::Result<()> {
    todo!("team B: pretty-printed, sorted keys")
}
