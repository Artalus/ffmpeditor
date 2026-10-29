use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Bare "ffmpeg" means look it up in PATH.
    pub ffmpeg_path: PathBuf,
}

impl AppConfig {
    pub fn load() -> Self {
        let Some(path) = config_path() else {
            return Self::default();
        };
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = config_path().context("no config directory on this system")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, toml::to_string_pretty(self)?)
            .with_context(|| format!("writing {}", path.display()))
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            ffmpeg_path: "ffmpeg".into(),
        }
    }
}

fn config_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "ffmpeditor")
        .map(|dirs| dirs.config_dir().join("config.toml"))
}
