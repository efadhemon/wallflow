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
    /// Id of the live wallpaper playing, if any. While set, the image rotation
    /// is suspended; it is started again after login.
    pub live_video: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            folders: Vec::new(),
            interval_seconds: 300,
            order: Order::Shuffle,
            fill_mode: FillMode::Fill,
            per_monitor: false,
            change_on_startup: true,
            excluded: Vec::new(),
            autostart: true,
            start_minimized: true,
            theme: Theme::System,
            live_video: None,
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
        write_atomic(&path, |temp| fs::write(temp, &raw).map_err(Into::into))
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    pub fn sanitize(&mut self) {
        self.version = CONFIG_VERSION;
        self.interval_seconds = self.interval_seconds.max(MIN_INTERVAL_SECONDS);
        // `dedup_by` only drops neighbours, and the list is in the user's order.
        let mut seen = std::collections::HashSet::new();
        self.folders
            .retain(|folder| seen.insert(folder.path.clone()));
    }

    pub fn is_excluded(&self, path: &Path) -> bool {
        self.excluded.iter().any(|p| p == path)
    }
}

/// Writes `path` by having `write` fill a hidden sibling file that is then renamed
/// over it, so a crash or a concurrent reader never sees half a file. The temp name
/// starts with a dot, which the scanner skips.
pub fn write_atomic(path: &Path, write: impl FnOnce(&Path) -> Result<()>) -> Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);

    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = path.with_file_name(format!(
        ".{name}.{}-{}.tmp",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));

    let result = write(&temp).and_then(|()| fs::rename(&temp, path).map_err(Into::into));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_folders_are_dropped_even_when_not_adjacent() {
        let folder = |path: &str| Folder {
            path: PathBuf::from(path),
            recursive: true,
        };
        let mut cfg = Config {
            folders: vec![folder("/a"), folder("/b"), folder("/a")],
            ..Config::default()
        };
        cfg.sanitize();
        assert_eq!(cfg.folders, vec![folder("/a"), folder("/b")]);
    }

    #[test]
    fn atomic_write_replaces_the_file_and_leaves_no_temp() {
        let dir = std::env::temp_dir().join(format!("wallflow-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("file.txt");
        fs::write(&path, "old").unwrap();

        write_atomic(&path, |temp| fs::write(temp, "new").map_err(Into::into)).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");

        let failed = write_atomic(&path, |_| anyhow::bail!("boom"));
        assert!(failed.is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

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
