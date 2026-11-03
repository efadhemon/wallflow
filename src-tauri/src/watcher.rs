use crate::state::{AppState, Command};
use notify::{RecursiveMode, Watcher};
use std::sync::Arc;

/// Watches every configured folder so images dropped in while the app is running
/// join the rotation without the user pressing anything. The scheduler debounces
/// the resulting bursts of events.
pub fn rewatch(state: &Arc<AppState>) {
    let tx = state.tx.clone();

    let mut watcher =
        match notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            // Reads (our own thumbnailing included) do not change what is on disk.
            if event.is_ok_and(|event| !event.kind.is_access()) {
                let _ = tx.send(Command::Rescan);
            }
        }) {
            Ok(watcher) => watcher,
            Err(err) => {
                log::warn!("folder watching is unavailable: {err}");
                return;
            }
        };

    for folder in state.config().folders {
        let mode = if folder.recursive {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        if let Err(err) = watcher.watch(&folder.path, mode) {
            log::warn!("could not watch {}: {err}", folder.path.display());
        }
    }

    if let Err(err) = watcher.watch(&state.images_dir, RecursiveMode::NonRecursive) {
        log::warn!("could not watch {}: {err}", state.images_dir.display());
    }

    // Dropping the previous watcher here also unregisters its old paths.
    *state.watcher.lock().expect("watcher poisoned") = Some(watcher);
}
