use crate::config::FillMode;
use anyhow::Result;
use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
mod gnome;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "linux")]
mod linux_live;
pub mod live;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
mod macos_live;
pub mod render;
#[cfg(target_os = "linux")]
mod wayland_live;
#[cfg(windows)]
pub mod windows;
#[cfg(windows)]
mod windows_live;

use live::{GnomeExtensionStatus, LiveOpts, LiveSupport};

pub trait WallpaperBackend: Send + Sync {
    /// Human readable name shown in the UI, e.g. "GNOME" or "Windows".
    fn name(&self) -> String;

    fn set(&self, path: &Path, mode: FillMode) -> Result<()>;

    /// Assign a different image to each monitor. `paths` is indexed by monitor.
    fn set_per_monitor(&self, paths: &[PathBuf], mode: FillMode) -> Result<()> {
        match paths.first() {
            Some(first) => self.set(first, mode),
            None => Ok(()),
        }
    }

    fn monitor_count(&self) -> usize {
        1
    }

    fn supports_per_monitor(&self) -> bool {
        false
    }

    /// Formats this platform can render as a wallpaper directly. Anything else is
    /// transcoded to PNG first — notably WebP, which neither Windows nor a stock
    /// GNOME install (without webp-pixbuf-loader) will display.
    fn native_extensions(&self) -> &'static [&'static str] {
        &["jpg", "jpeg", "png", "bmp"]
    }

    fn live_support(&self) -> LiveSupport {
        LiveSupport::None
    }

    fn gnome_extension_status(&self) -> GnomeExtensionStatus {
        GnomeExtensionStatus::NotApplicable
    }

    fn enable_gnome_extension(&self) -> Result<GnomeExtensionStatus> {
        anyhow::bail!("This desktop does not use GNOME Shell extensions.")
    }

    fn play_live(&self, _path: &Path, _opts: LiveOpts) -> Result<()> {
        live::unsupported()
    }

    fn stop_live(&self) {}

    fn pause_live(&self, _paused: bool) {}

    fn update_live_opts(&self, _opts: LiveOpts) {}

    fn extract_poster(&self, cache_dir: &Path, source: &Path) -> Result<PathBuf> {
        let target = live::poster_path(cache_dir, source)?;
        if target.is_file() {
            return Ok(target);
        }
        live::write_placeholder_poster(&target)?;
        Ok(target)
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
compile_error!("Wallflow supports Windows, macOS and Linux");

pub fn detect() -> Box<dyn WallpaperBackend> {
    #[cfg(windows)]
    {
        Box::new(windows::WindowsBackend::new())
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacosBackend::new())
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::LinuxBackend::new())
    }
}
