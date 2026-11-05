use crate::state::{now_ms, scan_images, AppState, Command};
use crate::{thumbs, tray};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::Instant;

pub const STATUS_EVENT: &str = "wallpaper://status";

/// How long to wait between wake-ups when there is nothing scheduled, so the UI
/// still reflects folders that appear later.
const IDLE_POLL: Duration = Duration::from_secs(30);

/// A freshly started session may not have its desktop shell up yet, which makes the
/// very first change fail. These are the gaps between retries.
const STARTUP_RETRY_DELAYS: [u64; 3] = [2, 5, 10];

/// Filesystem events come in bursts (a copy is create + many writes + close), so a
/// rescan waits until the folder has been quiet this long...
const RESCAN_QUIET: Duration = Duration::from_millis(1_000);
/// ...but never longer than this after the first event, so a long copy still shows
/// up while it is running.
const RESCAN_MAX_DELAY: Duration = Duration::from_secs(5);

/// A trailing debounce for watcher events. The last event of a burst always
/// leads to a rescan, so a file that finishes copying is never missed.
#[derive(Default)]
struct PendingRescan {
    first: Option<Instant>,
    due: Option<Instant>,
}

impl PendingRescan {
    fn bump(&mut self) {
        let now = Instant::now();
        let first = *self.first.get_or_insert(now);
        self.due = Some((now + RESCAN_QUIET).min(first + RESCAN_MAX_DELAY));
    }

    fn clear(&mut self) {
        *self = Self::default();
    }
}

pub async fn run(app: AppHandle, mut rx: UnboundedReceiver<Command>) {
    let state = app.state::<Arc<AppState>>().inner().clone();
    let mut pending = PendingRescan::default();

    rescan(&state, true).await;

    let change_on_startup = state.config().change_on_startup;
    if change_on_startup {
        apply_with_retry(&app, &state).await;
    } else {
        let mut inner = state.inner.lock().expect("state poisoned");
        inner.arm_timer();
        drop(inner);
        publish(&app, &state);
    }

    loop {
        let wait = wait_duration(&state);
        let wake_at = match pending.due {
            Some(due) => due.min(Instant::now() + wait),
            None => Instant::now() + wait,
        };

        tokio::select! {
            command = rx.recv() => {
                let Some(command) = command else {
                    log::info!("scheduler channel closed, stopping");
                    return;
                };
                handle(&app, &state, command, &mut pending).await;
            }
            _ = tokio::time::sleep_until(wake_at) => {
                if pending.due.is_some_and(|due| due <= Instant::now()) {
                    pending.clear();
                    rescan(&state, false).await;
                    publish(&app, &state);
                }

                let due = {
                    let inner = state.inner.lock().expect("state poisoned");
                    !inner.paused
                        && !inner.playlist.is_empty()
                        && inner.next_change_at.is_some_and(|at| at <= now_ms())
                };
                if due {
                    step(&app, &state, true).await;
                }
            }
        }
    }
}

async fn handle(
    app: &AppHandle,
    state: &Arc<AppState>,
    command: Command,
    pending: &mut PendingRescan,
) {
    match command {
        Command::Next => step(app, state, true).await,
        Command::Prev => step(app, state, false).await,
        Command::TogglePause => {
            {
                let mut inner = state.inner.lock().expect("state poisoned");
                inner.paused = !inner.paused;
                inner.arm_timer();
            }
            publish(app, state);
        }
        Command::Reload => {
            pending.clear();
            let images = rescan(state, true).await;
            state.inner.lock().expect("state poisoned").arm_timer();
            publish(app, state);

            // Settings changes are rare, so this is a good moment to drop cache
            // entries for images that are no longer in the rotation.
            let cache_dir = state.cache_dir.clone();
            tokio::task::spawn_blocking(move || thumbs::prune(&cache_dir, &images));
        }
        Command::Rescan => pending.bump(),
        Command::Refresh => {
            pending.clear();
            let was_empty = state
                .inner
                .lock()
                .expect("state poisoned")
                .playlist
                .is_empty();
            rescan(state, false).await;
            if was_empty {
                // The first images just arrived; start the countdown for them.
                state.inner.lock().expect("state poisoned").arm_timer();
            }
            publish(app, state);
        }
        Command::Rearm => {
            state.inner.lock().expect("state poisoned").arm_timer();
            publish(app, state);
        }
        Command::ApplyCurrent => {
            apply(app, state).await;
        }
    }
}

/// Walks the folders off the async runtime and without the state lock, so the UI
/// keeps answering while a large library is scanned. Returns everything found.
async fn rescan(state: &Arc<AppState>, force: bool) -> Vec<PathBuf> {
    let folders = state.config().folders;
    let images_dir = state.images_dir.clone();
    let images = tokio::task::spawn_blocking(move || scan_images(&folders, &images_dir))
        .await
        .unwrap_or_else(|err| {
            log::error!("scanning the folders failed: {err}");
            Vec::new()
        });

    let mut inner = state.inner.lock().expect("state poisoned");
    inner.rebuild_playlist(images.clone(), force);
    images
}

/// Move one image in the given direction and put it on screen.
async fn step(app: &AppHandle, state: &Arc<AppState>, forward: bool) {
    let empty = state
        .inner
        .lock()
        .expect("state poisoned")
        .playlist
        .is_empty();
    if empty {
        rescan(state, true).await;
    }

    {
        let mut inner = state.inner.lock().expect("state poisoned");
        if inner.playlist.is_empty() {
            inner.last_error = Some("No images found in the selected folders.".to_string());
            inner.arm_timer();
            drop(inner);
            publish(app, state);
            return;
        }
        inner.advance(forward);
    }
    apply(app, state).await;
}

async fn apply(app: &AppHandle, state: &Arc<AppState>) {
    match apply_once(state).await {
        Ok(()) => {
            let mut inner = state.inner.lock().expect("state poisoned");
            inner.last_error = None;
            inner.arm_timer();
        }
        Err(err) => {
            log::error!("could not set the wallpaper: {err:#}");
            let mut inner = state.inner.lock().expect("state poisoned");
            inner.last_error = Some(format!("{err:#}"));
            inner.arm_timer();
        }
    }
    publish(app, state);
}

/// Retries the first change of a session, because on a cold boot the desktop shell
/// may not be answering yet.
async fn apply_with_retry(app: &AppHandle, state: &Arc<AppState>) {
    {
        let mut inner = state.inner.lock().expect("state poisoned");
        if inner.playlist.is_empty() {
            // Nothing to retry; an empty library is not a desktop that is still starting.
            inner.arm_timer();
            drop(inner);
            publish(app, state);
            return;
        }
        if inner.current.is_none() {
            inner.advance(true);
        }
    }

    for (attempt, delay) in STARTUP_RETRY_DELAYS.iter().enumerate() {
        match apply_once(state).await {
            Ok(()) => {
                let mut inner = state.inner.lock().expect("state poisoned");
                inner.last_error = None;
                inner.arm_timer();
                drop(inner);
                publish(app, state);
                return;
            }
            Err(err) => {
                log::warn!(
                    "startup attempt {} failed, retrying in {delay}s: {err:#}",
                    attempt + 1
                );
                tokio::time::sleep(Duration::from_secs(*delay)).await;
            }
        }
    }

    apply(app, state).await;
}

/// Does the actual work off the async runtime: decoding and subprocesses both block.
async fn apply_once(state: &Arc<AppState>) -> anyhow::Result<()> {
    let state = state.clone();
    tokio::task::spawn_blocking(move || {
        // Asking the backend for the monitor count may call into the platform's UI
        // thread (macOS) or COM (Windows), so it must not happen while the state
        // lock is held — a UI-thread command waiting on that lock would deadlock.
        let per_monitor = state.config().per_monitor && state.backend.supports_per_monitor();
        let monitors = if per_monitor {
            state.backend.monitor_count()
        } else {
            1
        };

        let (targets, fill_mode) = {
            let inner = state.inner.lock().expect("state poisoned");
            let targets: Vec<PathBuf> = if per_monitor {
                inner.take_for_monitors(monitors)
            } else {
                inner.current.clone().into_iter().collect()
            };
            (targets, inner.config.fill_mode)
        };

        if targets.is_empty() {
            anyhow::bail!("No images found in the selected folders.");
        }

        let native = state.backend.native_extensions();
        let prepared: Vec<PathBuf> = targets
            .iter()
            .map(|path| thumbs::prepare_for_wallpaper(&state.cache_dir, path, native))
            .collect::<anyhow::Result<_>>()?;

        if per_monitor {
            state.backend.set_per_monitor(&prepared, fill_mode)
        } else {
            state.backend.set(&prepared[0], fill_mode)
        }
    })
    .await?
}

fn wait_duration(state: &Arc<AppState>) -> Duration {
    let inner = state.inner.lock().expect("state poisoned");
    if inner.paused || inner.playlist.is_empty() {
        return IDLE_POLL;
    }
    match inner.next_change_at {
        Some(at) => Duration::from_millis(at.saturating_sub(now_ms())).min(IDLE_POLL),
        None => IDLE_POLL,
    }
}

/// Push the current status to the UI and the tray in one go.
pub fn publish(app: &AppHandle, state: &Arc<AppState>) {
    let status = state.status();
    tray::update(app, &status);
    if let Err(err) = app.emit(STATUS_EVENT, &status) {
        log::debug!("no window to receive the status update: {err}");
    }
}
