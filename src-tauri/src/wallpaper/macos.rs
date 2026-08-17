use super::live::{LiveOpts, LiveSupport};
use super::render::{compose, ease_in_out_cubic, FADE_FRAME_MS, FADE_MS};
use super::WallpaperBackend;
use crate::config::FillMode;
use anyhow::{anyhow, Context, Result};
use dispatch2::run_on_main;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, ClassType, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSImage, NSImageScaling, NSImageView, NSScreen, NSView, NSWindow,
    NSWindowCollectionBehavior, NSWindowStyleMask, NSWorkspace,
    NSWorkspaceDesktopImageAllowClippingKey, NSWorkspaceDesktopImageOptionKey,
    NSWorkspaceDesktopImageScalingKey,
};
use objc2_foundation::{NSData, NSMutableDictionary, NSNumber, NSPoint, NSRect, NSSize, NSURL};
use std::cell::RefCell;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

/// AppKit desktop APIs must run on the main thread. The scheduler may call us
/// from a tokio worker, so hop over when needed. `run_on_main` is a no-op if
/// we are already there (e.g. a UI-thread IPC handler).
pub(super) fn on_main<F, R>(f: F) -> R
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

fn desktop_fill_mode(mode: FillMode) -> FillMode {
    match mode {
        FillMode::Tile => FillMode::Center,
        other => other,
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

pub(super) fn apply_on_screens(
    paths: &[PathBuf],
    mode: FillMode,
    mtm: MainThreadMarker,
) -> Result<()> {
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

pub(super) fn desktop_window_level() -> isize {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGWindowLevelForKey(key: i32) -> i32;
    }
    // kCGDesktopWindowLevelKey — public CoreGraphics constant.
    const DESKTOP_WINDOW_LEVEL_KEY: i32 = 2;
    unsafe { CGWindowLevelForKey(DESKTOP_WINDOW_LEVEL_KEY) as isize }
}

#[derive(Clone, Copy)]
pub(super) struct ScreenInfo {
    frame: NSRect,
    point_width: f64,
    point_height: f64,
    pixel_width: u32,
    pixel_height: u32,
}

pub(super) fn collect_screens(mtm: MainThreadMarker) -> Result<Vec<ScreenInfo>> {
    let screens = NSScreen::screens(mtm);
    if screens.is_empty() {
        return Err(anyhow!("no screens available"));
    }
    Ok(screens
        .iter()
        .map(|screen| {
            let frame = screen.frame();
            let scale = screen.backingScaleFactor().max(1.0);
            ScreenInfo {
                frame,
                point_width: frame.size.width,
                point_height: frame.size.height,
                pixel_width: (frame.size.width * scale).round().max(1.0) as u32,
                pixel_height: (frame.size.height * scale).round().max(1.0) as u32,
            }
        })
        .collect())
}

thread_local! {
    static OVERLAYS: RefCell<Vec<Retained<NSWindow>>> = RefCell::new(Vec::new());
}

fn close_overlays() {
    OVERLAYS.with(|slot| {
        for window in slot.borrow_mut().drain(..) {
            window.orderOut(None);
            window.close();
        }
    });
}

fn set_overlay_alpha(alpha: f64) {
    OVERLAYS.with(|slot| {
        for window in slot.borrow().iter() {
            window.setAlphaValue(alpha);
        }
    });
}

fn create_overlays(mtm: MainThreadMarker, frames: Vec<(Vec<u8>, ScreenInfo)>) -> Result<()> {
    close_overlays();
    let mut windows = Vec::with_capacity(frames.len());
    let level = desktop_window_level() + 1;

    for (png, info) in frames {
        let data = NSData::from_vec(png);
        let image = match NSImage::initWithData(NSImage::alloc(), &data) {
            Some(image) => image,
            None => {
                for window in windows {
                    window.orderOut(None);
                    window.close();
                }
                return Err(anyhow!("could not create the fade overlay image"));
            }
        };
        image.setSize(NSSize::new(info.point_width, info.point_height));

        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                info.frame,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe {
            window.setReleasedWhenClosed(false);
        }
        window.setLevel(level);
        window.setOpaque(false);
        window.setHasShadow(false);
        window.setIgnoresMouseEvents(true);
        window.setBackgroundColor(Some(&NSColor::clearColor()));
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        window.setAlphaValue(0.0);

        let content = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(info.point_width, info.point_height),
        );
        let view = NSImageView::initWithFrame(NSImageView::alloc(mtm), content);
        view.setImage(Some(&image));
        view.setImageScaling(NSImageScaling::ScaleAxesIndependently);
        let view: Retained<NSView> = Retained::into_super(Retained::into_super(view));
        window.setContentView(Some(&view));
        window.orderFrontRegardless();
        windows.push(window);
    }

    OVERLAYS.with(|slot| slot.replace(windows));
    Ok(())
}

fn encode_png(image: &image::RgbaImage) -> Result<Vec<u8>> {
    let mut png = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .context("encoding the fade overlay")?;
    Ok(png)
}

fn apply_with_fade(paths: &[PathBuf], mode: FillMode) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }

    let screens = match on_main(collect_screens) {
        Ok(screens) => screens,
        Err(err) => {
            log::warn!("could not read screens for the wallpaper fade: {err:#}");
            return on_main(|mtm| apply_on_screens(paths, mode, mtm));
        }
    };

    let compose_mode = desktop_fill_mode(mode);
    let mut frames = Vec::with_capacity(screens.len());
    for (index, screen) in screens.iter().enumerate() {
        let path = &paths[index % paths.len()];
        match compose(
            path,
            compose_mode,
            screen.pixel_width,
            screen.pixel_height,
            [0, 0, 0, 255],
        )
        .and_then(|image| encode_png(&image))
        {
            Ok(png) => frames.push((png, *screen)),
            Err(err) => {
                log::warn!("could not compose the wallpaper fade overlay: {err:#}");
                return on_main(|mtm| apply_on_screens(paths, mode, mtm));
            }
        }
    }

    if let Err(err) = on_main(move |mtm| create_overlays(mtm, frames)) {
        log::warn!("wallpaper fade overlay failed: {err:#}");
        on_main(|_| close_overlays());
        return on_main(|mtm| apply_on_screens(paths, mode, mtm));
    }

    let start = Instant::now();
    loop {
        let t = (start.elapsed().as_millis() as f32 / FADE_MS as f32).min(1.0);
        let alpha = ease_in_out_cubic(t) as f64;
        on_main(move |_| set_overlay_alpha(alpha));
        if t >= 1.0 {
            break;
        }
        thread::sleep(Duration::from_millis(FADE_FRAME_MS as u64));
    }

    let result = on_main(|mtm| apply_on_screens(paths, mode, mtm));
    on_main(|_| close_overlays());
    result
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
        self.stop_live();
        apply_with_fade(&[path.to_path_buf()], mode)
    }

    fn set_per_monitor(&self, paths: &[PathBuf], mode: FillMode) -> Result<()> {
        self.stop_live();
        apply_with_fade(paths, mode)
    }

    fn monitor_count(&self) -> usize {
        on_main(|mtm| NSScreen::screens(mtm).len().max(1))
    }

    fn supports_per_monitor(&self) -> bool {
        true
    }

    fn live_support(&self) -> LiveSupport {
        LiveSupport::Full
    }

    fn play_live(&self, path: &Path, opts: LiveOpts) -> Result<()> {
        super::macos_live::start(path, opts)
    }

    fn stop_live(&self) {
        super::macos_live::stop();
    }

    fn pause_live(&self, paused: bool) {
        super::macos_live::pause(paused);
    }

    fn update_live_opts(&self, opts: LiveOpts) {
        super::macos_live::update_opts(opts);
    }

    fn extract_poster(&self, cache_dir: &Path, source: &Path) -> Result<PathBuf> {
        super::macos_live::extract_poster(cache_dir, source)
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
        assert_eq!(desktop_fill_mode(FillMode::Tile), FillMode::Center);
    }
}
