use super::live::LiveOpts;
use super::macos::{collect_screens, desktop_window_level, on_main};
use anyhow::{anyhow, Context, Result};
use objc2::rc::Retained;
use objc2::{AnyThread, ClassType, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSBitmapImageFileType, NSBitmapImageRep, NSColor, NSImage, NSView,
    NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_av_foundation::{
    AVAssetImageGenerator, AVLayerVideoGravity, AVPlayerItem, AVPlayerLayer, AVPlayerLooper,
    AVQueuePlayer, AVURLAsset,
};
use objc2_core_media::CMTime;
use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize, NSURL};
use objc2_quartz_core::{CAAutoresizingMask, CALayer};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

struct Session {
    player: Retained<AVQueuePlayer>,
    #[allow(dead_code)]
    looper: Retained<AVPlayerLooper>,
    windows: Vec<Retained<NSWindow>>,
    layers: Vec<Retained<AVPlayerLayer>>,
    path: PathBuf,
    opts: LiveOpts,
}

thread_local! {
    static LIVE: RefCell<Option<Session>> = const { RefCell::new(None) };
}

static STOP_WATCH: AtomicBool = AtomicBool::new(true);
static WATCH_STARTED: OnceLock<()> = OnceLock::new();

fn ensure_display_watch() {
    WATCH_STARTED.get_or_init(|| {
        thread::Builder::new()
            .name("wallflow-live-screens".into())
            .spawn(|| loop {
                thread::sleep(Duration::from_secs(2));
                if STOP_WATCH.load(Ordering::SeqCst) {
                    continue;
                }
                on_main(|mtm| rebuild_if_needed(mtm));
            })
            .ok();
    });
}

pub fn start(path: &Path, opts: LiveOpts) -> Result<()> {
    let path = path.to_path_buf();
    on_main(move |mtm| {
        stop_on_main();
        let session = start_on_main(mtm, &path, opts)?;
        LIVE.with(|slot| *slot.borrow_mut() = Some(session));
        STOP_WATCH.store(false, Ordering::SeqCst);
        ensure_display_watch();
        Ok(())
    })
}

pub fn stop() {
    on_main(|_mtm| stop_on_main());
}

pub fn pause(paused: bool) {
    on_main(move |_| {
        LIVE.with(|slot| {
            if let Some(session) = slot.borrow().as_ref() {
                if paused {
                    session.player.pause();
                } else {
                    session.player.play();
                }
            }
        });
    });
}

pub fn update_opts(opts: LiveOpts) {
    on_main(move |_| {
        LIVE.with(|slot| {
            if let Some(session) = slot.borrow_mut().as_mut() {
                session.opts = opts;
                session.player.setMuted(opts.muted);
                let gravity = video_gravity(opts.fill_mode);
                for layer in &session.layers {
                    layer.setVideoGravity(gravity.clone());
                }
            }
        });
    });
}

fn stop_on_main() {
    STOP_WATCH.store(true, Ordering::SeqCst);
    LIVE.with(|slot| {
        if let Some(session) = slot.borrow_mut().take() {
            session.player.pause();
            for window in &session.windows {
                window.orderOut(None);
                window.close();
            }
        }
    });
}

fn rebuild_if_needed(mtm: objc2::MainThreadMarker) {
    let Some((path, opts, count)) = LIVE.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|session| (session.path.clone(), session.opts, session.windows.len()))
    }) else {
        return;
    };
    let Ok(screens) = collect_screens(mtm) else {
        return;
    };
    if screens.len() == count {
        return;
    }
    log::info!(
        "display layout changed ({} → {} screens), rebuilding live wallpaper",
        count,
        screens.len()
    );
    stop_on_main();
    match start_on_main(mtm, &path, opts) {
        Ok(session) => {
            LIVE.with(|slot| *slot.borrow_mut() = Some(session));
            STOP_WATCH.store(false, Ordering::SeqCst);
        }
        Err(err) => log::error!("could not rebuild live wallpaper: {err:#}"),
    }
}

fn video_gravity(mode: crate::config::FillMode) -> Retained<AVLayerVideoGravity> {
    match mode {
        crate::config::FillMode::Stretch => AVLayerVideoGravity::Resize,
        crate::config::FillMode::Fit
        | crate::config::FillMode::Center
        | crate::config::FillMode::Tile => AVLayerVideoGravity::ResizeAspect,
        crate::config::FillMode::Fill => AVLayerVideoGravity::ResizeAspectFill,
    }
}

fn start_on_main(mtm: objc2::MainThreadMarker, path: &Path, opts: LiveOpts) -> Result<Session> {
    let url = NSURL::from_file_path(path)
        .ok_or_else(|| anyhow!("could not convert {} to a file URL", path.display()))?;
    let item = unsafe { AVPlayerItem::initWithURL(AVPlayerItem::alloc(), &url) };
    let items = NSArray::from_retained_slice(&[item.clone()]);
    let player = unsafe { AVQueuePlayer::initWithItems(AVQueuePlayer::alloc(), &items) };
    let looper = unsafe { AVPlayerLooper::playerLooperWithPlayer_templateItem(&player, &item) };
    player.setMuted(opts.muted);
    player.play();

    let screens = collect_screens(mtm)?;
    let level = desktop_window_level();
    let gravity = video_gravity(opts.fill_mode);
    let mut windows = Vec::new();
    let mut layers = Vec::new();

    for screen in screens {
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                screen.frame,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe {
            window.setReleasedWhenClosed(false);
        }
        window.setLevel(level);
        window.setOpaque(true);
        window.setHasShadow(false);
        window.setIgnoresMouseEvents(true);
        window.setBackgroundColor(Some(&NSColor::blackColor()));
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );

        let content = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(screen.point_width, screen.point_height),
        );
        let view = NSView::initWithFrame(NSView::alloc(mtm), content);
        view.setWantsLayer(true);
        let layer = unsafe { AVPlayerLayer::playerLayerWithPlayer(Some(&player)) };
        layer.setVideoGravity(gravity.clone());
        layer.setFrame(content);
        layer.setAutoresizingMask(
            CAAutoresizingMask::LayerWidthSizable | CAAutoresizingMask::LayerHeightSizable,
        );
        if let Some(host) = view.layer() {
            let super_layer: &CALayer = layer.as_ref();
            host.addSublayer(super_layer);
        }
        window.setContentView(Some(&view));
        window.orderFrontRegardless();
        windows.push(window);
        layers.push(layer);
    }

    Ok(Session {
        player,
        looper,
        windows,
        layers,
        path: path.to_path_buf(),
        opts,
    })
}

pub fn extract_poster(cache_dir: &Path, source: &Path) -> Result<PathBuf> {
    let target = super::live::poster_path(cache_dir, source)?;
    if target.is_file() {
        return Ok(target);
    }

    let source = source.to_path_buf();
    let bytes: Result<Vec<u8>> = on_main(move |_| {
        let url = NSURL::from_file_path(&source)
            .ok_or_else(|| anyhow!("could not convert {} to a file URL", source.display()))?;
        let asset = unsafe { AVURLAsset::initWithURL_options(AVURLAsset::alloc(), &url, None) };
        let generator =
            unsafe { AVAssetImageGenerator::initWithAsset(AVAssetImageGenerator::alloc(), &asset) };
        generator.setAppliesPreferredTrackTransform(true);
        let image = unsafe {
            generator.copyCGImageAtTime_actualTime_error(CMTime::ZERO, std::ptr::null_mut())
        }
        .map_err(|err| anyhow!("reading a poster frame: {err}"))?;
        let nsimage =
            unsafe { NSImage::initWithCGImage_size(NSImage::alloc(), &image, NSSize::ZERO) };
        let tiff = nsimage
            .TIFFRepresentation()
            .ok_or_else(|| anyhow!("encoding the poster TIFF"))?;
        let rep = unsafe { NSBitmapImageRep::imageRepWithData(&tiff) }
            .ok_or_else(|| anyhow!("decoding the poster TIFF"))?;
        let data =
            unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::JPEG, None) }
                .ok_or_else(|| anyhow!("encoding the poster JPEG"))?;
        Ok(data.to_vec())
    });

    match bytes {
        Ok(bytes) => {
            std::fs::write(&target, bytes)
                .with_context(|| format!("writing {}", target.display()))?;
            Ok(target)
        }
        Err(err) => {
            log::warn!("could not extract a video poster: {err:#}");
            super::live::write_placeholder_poster(&target)?;
            Ok(target)
        }
    }
}
