use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Bare "ffmpeg" means look it up in PATH.
    pub ffmpeg_path: PathBuf,
}

impl AppConfig {
    /// A missing file is the first run, not a failure.
    pub fn load() -> anyhow::Result<Self> {
        let Some(path) = config_path() else {
            return Ok(Self::default());
        };
        Self::load_from(&path).with_context(|| format!("Config {} unreadable", path.display()))
    }

    fn load_from(path: &Path) -> anyhow::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(toml::from_str(&text)?),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(err.into()),
        }
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

pub fn config_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "ffmpeditor")
        .map(|dirs| dirs.config_dir().join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::output_path;

    #[test]
    fn only_a_missing_file_falls_back_to_defaults() {
        let path = output_path("config.toml");
        assert_eq!(AppConfig::load_from(&path).unwrap(), AppConfig::default());

        std::fs::write(&path, "ffmpeg_path = \"C:/ff/ffmpeg.exe\"").unwrap();
        let config = AppConfig::load_from(&path).unwrap();
        assert_eq!(config.ffmpeg_path, Path::new("C:/ff/ffmpeg.exe"));

        std::fs::write(&path, "ffmpeg_path = [").unwrap();
        assert!(AppConfig::load_from(&path).is_err());
        std::fs::remove_file(&path).unwrap();

        // Exists but cannot be read as a file.
        assert!(AppConfig::load_from(path.parent().unwrap()).is_err());
    }
}
