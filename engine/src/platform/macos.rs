//! macOS: one borderless window per screen at the desktop window level, on
//! every Space, ignoring the mouse. Covered windows are reported by AppKit as
//! occlusion changes (winit's `WindowEvent::Occluded`), so nothing is polled.

use super::Rect;
use anyhow::{Context, Result, bail};
use objc2_app_kit::{NSView, NSWindowCollectionBehavior};
use winit::event_loop::EventLoopBuilder;
use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

/// `kCGDesktopWindowLevelKey`: the level Finder draws the wallpaper at.
const DESKTOP_WINDOW_LEVEL_KEY: i32 = 2;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGWindowLevelForKey(key: i32) -> i32;
}

/// No Dock icon and no menu bar (`Prohibited` would also forbid windows).
pub fn configure_event_loop<T>(builder: &mut EventLoopBuilder<T>) {
    builder
        .with_activation_policy(ActivationPolicy::Accessory)
        .with_default_menu(false);
}

pub fn attach(window: &Window, rect: Rect) -> Result<()> {
    let RawWindowHandle::AppKit(handle) = window.window_handle()?.as_raw() else {
        bail!("not an AppKit window");
    };
    // SAFETY: winit hands out the NSView of a live window, on the main thread.
    let view: &NSView = unsafe { handle.ns_view.cast().as_ref() };
    let ns_window = view.window().context("the view has no window")?;

    let level = unsafe { CGWindowLevelForKey(DESKTOP_WINDOW_LEVEL_KEY) };
    ns_window.setLevel(level as isize);
    // On every Space, left alone by Mission Control and Cmd-Tab / window cycling.
    ns_window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    ns_window.setIgnoresMouseEvents(true);
    ns_window.setHasShadow(false);
    window.set_visible(true);
    log::info!(
        "wallpaper window at {},{} {}x{} on the desktop level",
        rect.x,
        rect.y,
        rect.width,
        rect.height
    );
    Ok(())
}
