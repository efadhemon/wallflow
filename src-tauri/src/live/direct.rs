//! The app runs the engine itself, as a child process: how Windows and macOS
//! work (the engine places its own windows behind the desktop there), and
//! on Linux with `WALLFLOW_LIVE_BACKEND=direct` for testing, where it opens
//! plain windows. The engine reports what it does with `--events`, and exits by
//! itself when its stdin closes, so it never outlives the app.

use anyhow::{Context, Result};
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Running {
    child: Child,
    stdin: ChildStdin,
    paused: Arc<AtomicBool>,
}

static ENGINE: Mutex<Option<Running>> = Mutex::new(None);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct State {
    pub running: bool,
    pub paused: bool,
}

/// Whether the engine still runs (an exited one is reaped here, so the
/// supervisor starts it again).
pub fn state() -> State {
    let mut engine = ENGINE.lock().expect("engine poisoned");
    let Some(running) = engine.as_mut() else {
        return State::default();
    };
    match running.child.try_wait() {
        Ok(None) => State {
            running: true,
            paused: running.paused.load(Ordering::Relaxed),
        },
        Ok(Some(status)) => {
            log::warn!("the live wallpaper engine exited ({status})");
            *engine = None;
            State::default()
        }
        Err(err) => {
            log::warn!("could not check the live wallpaper engine: {err}");
            State::default()
        }
    }
}

/// Starts `engine` with `args` (plus `--events`), replacing one that runs.
pub fn start(engine: &Path, args: &[OsString]) -> Result<()> {
    stop()?;
    let mut child = super::quiet_command(engine)
        .args(args)
        .arg("--events")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("starting {}", engine.display()))?;
    let stdin = child.stdin.take().context("the engine has no stdin")?;
    let stdout = child.stdout.take().context("the engine has no stdout")?;

    let paused = Arc::new(AtomicBool::new(false));
    let seen = paused.clone();
    std::thread::Builder::new()
        .name("wallflow-live-events".into())
        .spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                apply_event(&line, &seen);
            }
        })?;

    *ENGINE.lock().expect("engine poisoned") = Some(Running {
        child,
        stdin,
        paused,
    });
    Ok(())
}

/// One `--events` line from the engine.
fn apply_event(line: &str, paused: &AtomicBool) {
    let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
        return;
    };
    match event["event"].as_str() {
        Some("ready") => log::info!(
            "live wallpaper engine ready on {} screen(s)",
            event["screens"]
        ),
        Some("state") => paused.store(event["paused"] == true, Ordering::Relaxed),
        Some("error") => log::warn!("live wallpaper engine: {}", event["message"]),
        _ => {}
    }
}

/// Asks the engine to quit, and kills it if it has not within a second.
pub fn stop() -> Result<()> {
    let Some(mut running) = ENGINE.lock().expect("engine poisoned").take() else {
        return Ok(());
    };
    let _ = writeln!(running.stdin, r#"{{"cmd":"quit"}}"#);
    drop(running.stdin);
    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        if running.child.try_wait()?.is_some() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    log::warn!("the live wallpaper engine did not quit, killing it");
    running.child.kill()?;
    running.child.wait()?;
    Ok(())
}

/// One control line (`{"cmd":…}`) to the running engine.
pub fn send(line: &str) -> Result<()> {
    let mut engine = ENGINE.lock().expect("engine poisoned");
    let running = engine.as_mut().context("no live wallpaper is playing")?;
    writeln!(running.stdin, "{line}").context("the live wallpaper engine went away")?;
    running.stdin.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_events_set_paused() {
        let paused = AtomicBool::new(false);
        apply_event(r#"{"event":"state","paused":true,"reason":"app"}"#, &paused);
        assert!(paused.load(Ordering::Relaxed));
        apply_event(r#"{"event":"ready","screens":1}"#, &paused);
        assert!(paused.load(Ordering::Relaxed), "other events leave it");
        apply_event(
            r#"{"event":"state","paused":false,"reason":"app"}"#,
            &paused,
        );
        assert!(!paused.load(Ordering::Relaxed));
        apply_event("not json", &paused);
        assert!(!paused.load(Ordering::Relaxed));
    }
}
