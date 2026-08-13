use crate::state::{now_ms, AppState, Command};
use crate::{thumbs, tray};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc::UnboundedReceiver;

pub const STATUS_EVENT: &str = "wallpaper://status";

/// How long to wait between wake-ups when there is nothing scheduled, so the UI
/// still reflects folders that appear later.
const IDLE_POLL: Duration = Duration::from_secs(30);

/// A freshly started session may not have its desktop shell up yet, which makes the
/// very first change fail. These are the gaps between retries.
const STARTUP_RETRY_DELAYS: [u64; 3] = [2, 5, 10];

/// Ignore filesystem events that arrive on the heels of a rescan we just did.
const RESCAN_DEBOUNCE_MS: u64 = 2_000;

pub async fn run(app: AppHandle, mut rx: UnboundedReceiver<Command>) {
    let state = app.state::<Arc<AppState>>().inner().clone();
    let mut last_rescan = 0u64;

    {
        let mut inner = state.inner.lock().expect("state poisoned");
        inner.rebuild_playlist(true);
    }

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

        tokio::select! {
            command = rx.recv() => {
                let Some(command) = command else {
                    log::info!("scheduler channel closed, stopping");
                    return;
                };
                handle(&app, &state, command, &mut last_rescan).await;
            }
            _ = tokio::time::sleep(wait) => {
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

async fn handle(app: &AppHandle, state: &Arc<AppState>, command: Command, last_rescan: &mut u64) {
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
            let images = {
                let mut inner = state.inner.lock().expect("state poisoned");
                inner.rebuild_playlist(true);
                inner.arm_timer();
                inner.images.clone()
            };
            *last_rescan = now_ms();
            publish(app, state);

            // Settings changes are rare, so this is a good moment to drop cache
            // entries for images that are no longer in the rotation.
            let cache_dir = state.cache_dir.clone();
            tokio::task::spawn_blocking(move || thumbs::prune(&cache_dir, &images));
        }
        Command::Rescan => {
            if now_ms().saturating_sub(*last_rescan) < RESCAN_DEBOUNCE_MS {
                return;
            }
            *last_rescan = now_ms();
            {
                let mut inner = state.inner.lock().expect("state poisoned");
                inner.rebuild_playlist(false);
            }
            publish(app, state);
        }
        Command::ApplyCurrent => {
            apply(app, state).await;
        }
    }
}

/// Move one image in the given direction and put it on screen.
async fn step(app: &AppHandle, state: &Arc<AppState>, forward: bool) {
    {
        let mut inner = state.inner.lock().expect("state poisoned");
        if inner.playlist.is_empty() {
            inner.rebuild_playlist(true);
        }
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
    let (targets, fill_mode, per_monitor) = {
        let inner = state.inner.lock().expect("state poisoned");
        let per_monitor = inner.config.per_monitor && state.backend.supports_per_monitor();
        let targets: Vec<PathBuf> = if per_monitor {
            inner.take_for_monitors(state.backend.monitor_count())
        } else {
            inner.current.clone().into_iter().collect()
        };
        (targets, inner.config.fill_mode, per_monitor)
    };

    if targets.is_empty() {
        anyhow::bail!("No images found in the selected folders.");
    }

    let state = state.clone();
    tokio::task::spawn_blocking(move || {
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
