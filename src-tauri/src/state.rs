use crate::config::{Config, Folder, Order};
use crate::scanner;
use crate::wallpaper::WallpaperBackend;
use notify::RecommendedWatcher;
use rand::seq::SliceRandom;
use rand::RngExt;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc::UnboundedSender;

#[derive(Debug, Clone, Copy)]
pub enum Command {
    /// Move forward one image and apply it.
    Next,
    /// Move back one image and apply it.
    Prev,
    TogglePause,
    /// Config changed: rebuild the playlist and restart the countdown.
    Reload,
    /// Files on disk changed: refresh the playlist but leave the countdown alone.
    Rescan,
    /// Re-apply whatever is current, e.g. after picking an image in the UI.
    ApplyCurrent,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageInfo {
    pub path: PathBuf,
    pub name: String,
    pub folder: String,
}

impl ImageInfo {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            folder: path
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub current: Option<ImageInfo>,
    pub paused: bool,
    /// Unix milliseconds, or null when paused or idle.
    pub next_change_at: Option<u64>,
    pub interval_seconds: u64,
    pub total_images: usize,
    pub playlist_size: usize,
    pub position: usize,
    pub last_error: Option<String>,
}

pub struct Inner {
    pub config: Config,
    /// Everything found on disk, including excluded images.
    pub images: Vec<PathBuf>,
    /// What the rotation actually walks through, in play order.
    pub playlist: Vec<PathBuf>,
    pub index: usize,
    pub current: Option<PathBuf>,
    pub paused: bool,
    pub next_change_at: Option<u64>,
    pub last_error: Option<String>,
}

impl Inner {
    /// Rescans the folders plus the managed images directory and rebuilds the
    /// play order. `force` is for settings changes; without it an unchanged set
    /// of paths keeps its existing order, so a filesystem event cannot restart
    /// a shuffle pass halfway through.
    pub fn rebuild_playlist(&mut self, images_dir: &Path, force: bool) {
        let mut images = scanner::scan(&self.config.folders);
        images.extend(scanner::scan(&[Folder {
            path: images_dir.to_path_buf(),
            recursive: false,
        }]));
        images.sort();
        images.dedup();
        self.images = images;

        // `scan` returns sorted paths, so this stays sorted after filtering.
        let mut candidates: Vec<PathBuf> = self
            .images
            .iter()
            .filter(|path| !self.config.is_excluded(path))
            .cloned()
            .collect();

        if !force && !self.playlist.is_empty() {
            let mut existing = self.playlist.clone();
            existing.sort();
            if existing == candidates {
                return;
            }
        }

        if self.config.order == Order::Shuffle {
            candidates.shuffle(&mut rand::rng());
        }

        // Keep showing the same image across a rebuild when it survived the change.
        self.index = self
            .current
            .as_ref()
            .and_then(|current| candidates.iter().position(|path| path == current))
            .unwrap_or(0);

        self.playlist = candidates;

        if self.playlist.is_empty() {
            self.current = None;
        } else if !self
            .current
            .as_ref()
            .is_some_and(|current| self.playlist.contains(current))
        {
            self.current = self.playlist.get(self.index).cloned();
        }
    }

    pub fn advance(&mut self, forward: bool) {
        let len = self.playlist.len();
        if len == 0 {
            self.current = None;
            return;
        }

        match self.config.order {
            Order::Random if len > 1 => {
                let mut rng = rand::rng();
                let mut next = self.index;
                while next == self.index {
                    next = rng.random_range(0..len);
                }
                self.index = next;
            }
            Order::Shuffle => {
                if forward {
                    if self.index + 1 >= len {
                        // A full pass is done; reshuffle so the next pass differs.
                        self.playlist.shuffle(&mut rand::rng());
                        self.index = 0;
                    } else {
                        self.index += 1;
                    }
                } else {
                    self.index = if self.index == 0 {
                        len - 1
                    } else {
                        self.index - 1
                    };
                }
            }
            _ => {
                self.index = if forward {
                    (self.index + 1) % len
                } else if self.index == 0 {
                    len - 1
                } else {
                    self.index - 1
                };
            }
        }

        self.current = self.playlist.get(self.index).cloned();
    }

    /// Distinct images for a multi-monitor setup, starting at the current one.
    pub fn take_for_monitors(&self, count: usize) -> Vec<PathBuf> {
        if self.playlist.is_empty() {
            return Vec::new();
        }
        (0..count)
            .map(|offset| self.playlist[(self.index + offset) % self.playlist.len()].clone())
            .collect()
    }

    pub fn arm_timer(&mut self) {
        self.next_change_at = if self.paused || self.playlist.is_empty() {
            None
        } else {
            Some(now_ms() + self.config.interval_seconds * 1000)
        };
    }

    pub fn status(&self) -> Status {
        Status {
            current: self.current.as_deref().map(ImageInfo::new),
            paused: self.paused,
            next_change_at: self.next_change_at,
            interval_seconds: self.config.interval_seconds,
            total_images: self.images.len(),
            playlist_size: self.playlist.len(),
            position: if self.playlist.is_empty() {
                0
            } else {
                self.index + 1
            },
            last_error: self.last_error.clone(),
        }
    }
}

pub struct AppState {
    pub inner: Mutex<Inner>,
    pub backend: Box<dyn WallpaperBackend>,
    pub config_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub log_dir: PathBuf,
    /// Copies of images added one-by-one, scanned alongside `config.folders`.
    pub images_dir: PathBuf,
    pub tx: UnboundedSender<Command>,
    pub watcher: Mutex<Option<RecommendedWatcher>>,
}

impl AppState {
    pub fn send(&self, command: Command) {
        if let Err(err) = self.tx.send(command) {
            log::error!("scheduler is not running: {err}");
        }
    }

    pub fn status(&self) -> Status {
        self.inner.lock().expect("state poisoned").status()
    }

    pub fn config(&self) -> Config {
        self.inner.lock().expect("state poisoned").config.clone()
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
