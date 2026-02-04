use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
pub struct Config {
    pub(crate) theme: Option<String>,
}

pub fn load_config() -> Result<Config> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    candidates.push(PathBuf::from("rvim.toml"));
    candidates.push(PathBuf::from(".rvim.toml"));
    if let Ok(home) = std::env::var("HOME") {
        candidates.push(PathBuf::from(home).join(".config/rvim/config.toml"));
    }

    for path in candidates {
        if !path.exists() {
            continue;
        }
        let content = fs::read_to_string(&path)?;
        let cfg: Config = toml::from_str(&content)?;
        return Ok(cfg);
    }
    Ok(Config::default())
}
