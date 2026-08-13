use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const CONFIG_VERSION: u32 = 1;
pub const CONFIG_FILE: &str = "config.json";

/// Anything shorter than this hammers the desktop environment for no benefit.
pub const MIN_INTERVAL_SECONDS: u64 = 5;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub path: PathBuf,
    #[serde(default = "yes")]
    pub recursive: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Order {
    /// Walk a shuffled playlist, reshuffling only once every image has been shown.
    #[default]
    Shuffle,
    /// Alphabetical, in scan order.
    Sequential,
    /// Independent pick every time, repeats allowed.
    Random,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum FillMode {
    #[default]
    Fill,
    Fit,
    Stretch,
    Center,
    Tile,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Light,
    Dark,
    #[default]
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    pub version: u32,
    pub folders: Vec<Folder>,
    pub interval_seconds: u64,
    pub order: Order,
    pub fill_mode: FillMode,
    pub per_monitor: bool,
    pub change_on_startup: bool,
    pub excluded: Vec<PathBuf>,
    pub autostart: bool,
    pub start_minimized: bool,
    pub theme: Theme,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            folders: default_folders(),
            interval_seconds: 300,
            order: Order::Shuffle,
            fill_mode: FillMode::Fill,
            per_monitor: false,
            change_on_startup: true,
            excluded: Vec::new(),
            autostart: true,
            start_minimized: true,
            theme: Theme::System,
        }
    }
}

impl Config {
    pub fn load(config_dir: &Path) -> Self {
        let path = config_dir.join(CONFIG_FILE);
        match fs::read_to_string(&path) {
            Ok(raw) => match serde_json::from_str::<Config>(&raw) {
                Ok(mut cfg) => {
                    cfg.sanitize();
                    cfg
                }
                Err(err) => {
                    log::error!(
                        "config at {} is invalid, using defaults: {err}",
                        path.display()
                    );
                    Config::default()
                }
            },
            Err(_) => Config::default(),
        }
    }

    pub fn save(&self, config_dir: &Path) -> Result<()> {
        fs::create_dir_all(config_dir)
            .with_context(|| format!("creating config dir {}", config_dir.display()))?;
        let path = config_dir.join(CONFIG_FILE);
        let raw = serde_json::to_string_pretty(self)?;
        fs::write(&path, raw).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    pub fn sanitize(&mut self) {
        self.version = CONFIG_VERSION;
        self.interval_seconds = self.interval_seconds.max(MIN_INTERVAL_SECONDS);
        self.folders.dedup_by(|a, b| a.path == b.path);
    }

    pub fn is_excluded(&self, path: &Path) -> bool {
        self.excluded.iter().any(|p| p == path)
    }
}

pub fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    let key = "USERPROFILE";
    #[cfg(not(windows))]
    let key = "HOME";
    std::env::var_os(key)
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

fn default_folders() -> Vec<Folder> {
    let Some(home) = home_dir() else {
        return Vec::new();
    };
    for candidate in ["Pictures/wallpapers", "Pictures/Wallpapers", "Pictures"] {
        let path = home.join(candidate);
        if path.is_dir() {
            return vec![Folder {
                path,
                recursive: true,
            }];
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interval_is_clamped() {
        let mut cfg = Config {
            interval_seconds: 0,
            ..Config::default()
        };
        cfg.sanitize();
        assert_eq!(cfg.interval_seconds, MIN_INTERVAL_SECONDS);
    }
}
