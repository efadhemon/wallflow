//! Putting the wallpaper windows behind the desktop, and noticing when they are
//! covered, on systems where the engine does that itself (Windows, macOS).
//!
//! On Linux the GNOME shim places the windows and stops asking for frames while
//! the desktop is hidden, so there is nothing to do here.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

use anyhow::Result;
use std::time::Duration;
use winit::event_loop::EventLoopBuilder;
use winit::window::{Window, WindowAttributes};

/// How often [`check`] runs while playing in the background.
pub const CHECK_EVERY: Duration = Duration::from_secs(1);

/// A monitor's area in physical pixels, in desktop coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    #[cfg_attr(not(any(windows, test)), allow(dead_code))]
    pub fn contains(&self, other: &Rect) -> bool {
        self.x <= other.x
            && self.y <= other.y
            && self.x + self.width as i32 >= other.x + other.width as i32
            && self.y + self.height as i32 >= other.y + other.height as i32
    }
}

/// Event loop settings for background mode (macOS: no Dock icon, no menu bar).
pub fn configure_event_loop<T>(builder: &mut EventLoopBuilder<T>) {
    #[cfg(target_os = "macos")]
    macos::configure_event_loop(builder);
    #[cfg(not(target_os = "macos"))]
    let _ = builder;
}

/// Window settings for a background window, before it is created.
pub fn background_attributes(attrs: WindowAttributes) -> WindowAttributes {
    #[cfg(windows)]
    let attrs = windows::background_attributes(attrs);
    // Shown once it is behind the desktop, so it never flashes over the screen.
    #[cfg(any(windows, target_os = "macos"))]
    let attrs = attrs.with_visible(false);
    attrs
}

/// Whether the engine makes the window click-through itself. On Windows the
/// desktop icons sit above it anyway, and winit's click-through styles would
/// fight the ones the desktop needs.
pub const CLICK_THROUGH: bool = cfg!(not(windows));

/// Puts one monitor-sized window behind the desktop icons and shows it.
pub fn attach(window: &Window, rect: Rect) -> Result<()> {
    #[cfg(windows)]
    return windows::attach(window, rect);
    #[cfg(target_os = "macos")]
    return macos::attach(window, rect);
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (window, rect);
        Ok(())
    }
}

/// Whether [`check`] needs to run at all (macOS reports covered windows as
/// occlusion events instead).
pub const NEEDS_CHECKS: bool = cfg!(windows);

/// Puts windows back where Explorer moved them from, and says which ones are
/// covered (a maximized or full-screen window, the lock screen).
pub fn check(windows: &[(&Window, Rect)]) -> Vec<bool> {
    #[cfg(windows)]
    return windows::check(windows);
    #[cfg(not(windows))]
    vec![false; windows.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_contains() {
        let screen = Rect {
            x: -1920,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let bigger = Rect {
            x: -1928,
            y: -8,
            width: 1936,
            height: 1096,
        };
        let smaller = Rect {
            x: -1920,
            y: 0,
            width: 1920,
            height: 1040,
        };
        assert!(screen.contains(&screen));
        assert!(bigger.contains(&screen));
        assert!(!smaller.contains(&screen));
        assert!(!screen.contains(&bigger));
    }
}
