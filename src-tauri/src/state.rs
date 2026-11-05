use crate::config::{Config, Folder, Order};
use crate::scanner;
use crate::wallpaper::WallpaperBackend;
use notify::RecommendedWatcher;
use rand::seq::SliceRandom;
use rand::RngExt;
use serde::Serialize;
use std::collections::HashSet;
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
    /// Debounced, since the watcher sends one per filesystem event.
    Rescan,
    /// Like `Rescan`, but sent by the app itself after it added, removed or
    /// excluded an image, so it runs straight away.
    Refresh,
    /// Only the interval changed: restart the countdown, keep everything else.
    Rearm,
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
    /// Id of the live wallpaper playing instead of the rotation, if any.
    pub live: Option<String>,
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

/// Every image in the configured folders plus the managed images directory,
/// sorted. This walks the disk, so call it without holding the state lock.
pub fn scan_images(folders: &[Folder], images_dir: &Path) -> Vec<PathBuf> {
    let mut images = scanner::scan(folders);
    images.extend(scanner::scan(&[Folder {
        path: images_dir.to_path_buf(),
        recursive: false,
    }]));
    images.sort();
    images.dedup();
    images
}

impl Inner {
    /// Rebuilds the play order from a fresh `scan_images` result. `force` is for
    /// settings changes; without it the images that survived keep their place,
    /// so a filesystem event cannot restart a shuffle pass halfway through.
    pub fn rebuild_playlist(&mut self, images: Vec<PathBuf>, force: bool) {
        self.images = images;

        // `scan_images` returns sorted paths, so this stays sorted after filtering.
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
            if !force && !self.playlist.is_empty() {
                // Keep the pass going: survivors stay in order, newcomers are
                // shuffled in after them.
                let wanted: HashSet<&PathBuf> = candidates.iter().collect();
                let known: HashSet<&PathBuf> = self.playlist.iter().collect();
                let mut fresh: Vec<PathBuf> = candidates
                    .iter()
                    .filter(|path| !known.contains(path))
                    .cloned()
                    .collect();
                fresh.shuffle(&mut rand::rng());
                let mut kept: Vec<PathBuf> = self
                    .playlist
                    .iter()
                    .filter(|path| wanted.contains(path))
                    .cloned()
                    .collect();
                kept.extend(fresh);
                candidates = kept;
            } else {
                candidates.shuffle(&mut rand::rng());
            }
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
            live: self.config.live_video.clone(),
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
    pub live: crate::live::Live,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn inner(order: Order) -> Inner {
        Inner {
            config: Config {
                order,
                ..Config::default()
            },
            images: Vec::new(),
            playlist: Vec::new(),
            index: 0,
            current: None,
            paused: false,
            next_change_at: None,
            last_error: None,
        }
    }

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names
            .iter()
            .map(|name| PathBuf::from(format!("/w/{name}")))
            .collect()
    }

    #[test]
    fn soft_rebuild_keeps_the_shuffle_pass_and_appends_newcomers() {
        let mut state = inner(Order::Shuffle);
        state.rebuild_playlist(paths(&["a", "b", "c", "d"]), true);
        state.advance(true);
        let before = state.playlist.clone();
        let current = state.current.clone();

        state.config.excluded.push(before[3].clone());
        state.rebuild_playlist(paths(&["a", "b", "c", "d", "e"]), false);

        let expected: Vec<PathBuf> = before[..3].iter().cloned().chain(paths(&["e"])).collect();
        assert_eq!(state.playlist, expected);
        assert_eq!(state.current, current);
        assert_eq!(state.playlist[state.index], current.unwrap());
    }

    #[test]
    fn soft_rebuild_keeps_sequential_order_sorted() {
        let mut state = inner(Order::Sequential);
        state.rebuild_playlist(paths(&["b", "d"]), true);
        state.rebuild_playlist(paths(&["a", "b", "c", "d"]), false);
        assert_eq!(state.playlist, paths(&["a", "b", "c", "d"]));
    }

    #[test]
    fn removing_the_current_image_moves_to_a_remaining_one() {
        let mut state = inner(Order::Sequential);
        state.rebuild_playlist(paths(&["a", "b"]), true);
        state.current = Some(PathBuf::from("/w/b"));
        state.rebuild_playlist(paths(&["a"]), false);
        assert_eq!(state.current, Some(PathBuf::from("/w/a")));
    }
}
