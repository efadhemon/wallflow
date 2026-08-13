use super::WallpaperBackend;
use crate::config::FillMode;
use anyhow::{anyhow, Result};
use dispatch2::run_on_main;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{ClassType, MainThreadMarker};
use objc2_app_kit::{
    NSImageScaling, NSScreen, NSWorkspace, NSWorkspaceDesktopImageAllowClippingKey,
    NSWorkspaceDesktopImageOptionKey, NSWorkspaceDesktopImageScalingKey,
};
use objc2_foundation::{NSMutableDictionary, NSNumber, NSURL};
use std::path::{Path, PathBuf};

/// AppKit desktop APIs must run on the main thread. The scheduler may call us
/// from a tokio worker, so hop over when needed. `run_on_main` is a no-op if
/// we are already there (e.g. a UI-thread IPC handler).
fn on_main<F, R>(f: F) -> R
where
    F: Send + FnOnce(MainThreadMarker) -> R,
    R: Send,
{
    run_on_main(f)
}

/// macOS has no wallpaper tile mode; Tile is treated as Center.
fn scaling_and_clip(mode: FillMode) -> (NSImageScaling, bool) {
    match mode {
        FillMode::Fill => (NSImageScaling::ScaleProportionallyUpOrDown, true),
        FillMode::Fit => (NSImageScaling::ScaleProportionallyUpOrDown, false),
        FillMode::Stretch => (NSImageScaling::ScaleAxesIndependently, true),
        FillMode::Center | FillMode::Tile => (NSImageScaling::ScaleNone, false),
    }
}

fn desktop_options(
    mode: FillMode,
) -> Retained<NSMutableDictionary<NSWorkspaceDesktopImageOptionKey, AnyObject>> {
    let (scaling, clip) = scaling_and_clip(mode);
    let scaling_num = NSNumber::new_u64(scaling.0 as u64);
    let clip_num = NSNumber::new_bool(clip);
    let options = NSMutableDictionary::new();
    let scaling_obj: &AnyObject = AsRef::as_ref(&*scaling_num);
    let clip_obj: &AnyObject = AsRef::as_ref(&*clip_num);
    // SAFETY: these are the documented NSWorkspaceDesktopImageOptionKey constants.
    unsafe {
        options.insert(NSWorkspaceDesktopImageScalingKey, scaling_obj);
        options.insert(NSWorkspaceDesktopImageAllowClippingKey, clip_obj);
    }
    options
}

fn file_url(path: &Path) -> Result<Retained<NSURL>> {
    NSURL::from_file_path(path)
        .ok_or_else(|| anyhow!("could not convert {} to a file URL", path.display()))
}

fn apply_on_screens(paths: &[PathBuf], mode: FillMode, mtm: MainThreadMarker) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }

    let screens = NSScreen::screens(mtm);
    if screens.is_empty() {
        return Err(anyhow!("no screens available"));
    }

    let workspace = NSWorkspace::sharedWorkspace();
    let options = desktop_options(mode);

    for (index, screen) in screens.iter().enumerate() {
        let path = &paths[index % paths.len()];
        let url = file_url(path)?;
        // SAFETY: `options` only contains NSNumber values for the documented
        // scaling and clipping keys, which is what AppKit expects.
        unsafe {
            workspace
                .setDesktopImageURL_forScreen_options_error(&url, &screen, options.as_super())
                .map_err(|err| {
                    anyhow!(
                        "setting the wallpaper on screen {index} to {}: {}",
                        path.display(),
                        *err
                    )
                })?;
        }
    }

    Ok(())
}

pub struct MacosBackend;

impl MacosBackend {
    pub fn new() -> Self {
        Self
    }
}

impl WallpaperBackend for MacosBackend {
    fn name(&self) -> String {
        "macOS".to_string()
    }

    fn set(&self, path: &Path, mode: FillMode) -> Result<()> {
        let path = path.to_path_buf();
        on_main(move |mtm| apply_on_screens(&[path], mode, mtm))
    }

    fn set_per_monitor(&self, paths: &[PathBuf], mode: FillMode) -> Result<()> {
        if paths.is_empty() {
            return Ok(());
        }
        let paths = paths.to_vec();
        on_main(move |mtm| apply_on_screens(&paths, mode, mtm))
    }

    fn monitor_count(&self) -> usize {
        on_main(|mtm| NSScreen::screens(mtm).len().max(1))
    }

    fn supports_per_monitor(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_clips_while_fitting_proportionally() {
        let (scaling, clip) = scaling_and_clip(FillMode::Fill);
        assert_eq!(scaling, NSImageScaling::ScaleProportionallyUpOrDown);
        assert!(clip);
    }

    #[test]
    fn fit_does_not_clip() {
        let (scaling, clip) = scaling_and_clip(FillMode::Fit);
        assert_eq!(scaling, NSImageScaling::ScaleProportionallyUpOrDown);
        assert!(!clip);
    }

    #[test]
    fn stretch_scales_axes_independently() {
        let (scaling, clip) = scaling_and_clip(FillMode::Stretch);
        assert_eq!(scaling, NSImageScaling::ScaleAxesIndependently);
        assert!(clip);
    }

    #[test]
    fn tile_falls_back_to_center() {
        assert_eq!(
            scaling_and_clip(FillMode::Tile),
            scaling_and_clip(FillMode::Center)
        );
        let (scaling, clip) = scaling_and_clip(FillMode::Center);
        assert_eq!(scaling, NSImageScaling::ScaleNone);
        assert!(!clip);
    }
}
