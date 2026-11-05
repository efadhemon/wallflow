//! `--events`: what the engine is doing, one JSON object per stdout line, for
//! an app that starts the engine itself (Windows, macOS, and the `direct`
//! backend on Linux). Off by default: the GNOME shim does not read stdout, so
//! the lines would only fill the journal.

use serde::Serialize;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Serialize, PartialEq)]
#[serde(tag = "event", rename_all = "camelCase")]
pub enum Event<'a> {
    /// Windows are open and the first frame is on its way.
    Ready { screens: usize },
    /// Playback paused or resumed; `reason` says who asked (`app`, …).
    State { paused: bool, reason: &'a str },
    /// The engine is about to exit because of this.
    Error { message: &'a str },
}

pub fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
}

pub fn emit(event: &Event) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    if let Ok(line) = serde_json::to_string(event) {
        let mut out = std::io::stdout().lock();
        // A reader that went away is not worth failing playback over.
        let _ = writeln!(out, "{line}").and_then(|()| out.flush());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_tagged_json() {
        let line = |event: &Event| serde_json::to_string(event).unwrap();
        assert_eq!(
            line(&Event::Ready { screens: 2 }),
            r#"{"event":"ready","screens":2}"#
        );
        assert_eq!(
            line(&Event::State {
                paused: true,
                reason: "app"
            }),
            r#"{"event":"state","paused":true,"reason":"app"}"#
        );
        assert_eq!(
            line(&Event::Error { message: "no GPU" }),
            r#"{"event":"error","message":"no GPU"}"#
        );
    }
}
