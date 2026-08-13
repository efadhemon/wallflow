use super::WallpaperBackend;
use crate::config::FillMode;
use anyhow::{Context, Result};
use std::cell::Cell;
use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Shell::{
    DesktopWallpaper, IDesktopWallpaper, DESKTOP_WALLPAPER_POSITION, DWPOS_CENTER, DWPOS_FILL,
    DWPOS_FIT, DWPOS_STRETCH, DWPOS_TILE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, SPIF_SENDWININICHANGE, SPIF_UPDATEINIFILE, SPI_SETDESKWALLPAPER,
};

thread_local! {
    static COM_READY: Cell<bool> = const { Cell::new(false) };
}

/// COM must be initialised on every thread that touches it, and the scheduler
/// runs on whichever tokio worker picks up the task.
fn ensure_com() {
    COM_READY.with(|ready| {
        if !ready.get() {
            // A failure here means COM is already initialised in another mode,
            // which is still usable for our purposes.
            let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            ready.set(true);
        }
    });
}

fn position(mode: FillMode) -> DESKTOP_WALLPAPER_POSITION {
    match mode {
        FillMode::Fill => DWPOS_FILL,
        FillMode::Fit => DWPOS_FIT,
        FillMode::Stretch => DWPOS_STRETCH,
        FillMode::Center => DWPOS_CENTER,
        FillMode::Tile => DWPOS_TILE,
    }
}

fn interface() -> Result<IDesktopWallpaper> {
    ensure_com();
    let wallpaper: IDesktopWallpaper =
        unsafe { CoCreateInstance(&DesktopWallpaper, None, CLSCTX_ALL) }
            .context("creating the IDesktopWallpaper COM object")?;
    Ok(wallpaper)
}

fn monitor_ids(wallpaper: &IDesktopWallpaper) -> Result<Vec<HSTRING>> {
    let count = unsafe { wallpaper.GetMonitorDevicePathCount() }.context("counting monitors")?;

    let mut ids = Vec::with_capacity(count as usize);
    for index in 0..count {
        let raw = unsafe { wallpaper.GetMonitorDevicePathAt(index) }
            .with_context(|| format!("reading device path of monitor {index}"))?;
        if raw.is_null() {
            continue;
        }
        let id = unsafe { raw.to_hstring() };
        unsafe { CoTaskMemFree(Some(raw.0 as *const c_void)) };
        // Disconnected monitors are still enumerated but have an empty path.
        if !id.is_empty() {
            ids.push(id);
        }
    }
    Ok(ids)
}

/// Pre-Vista API that only ever sets one image across the whole desktop. Used when
/// the COM object is unavailable, which mostly happens on stripped-down Windows images.
fn set_via_system_parameters(path: &Path) -> Result<()> {
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);

    unsafe {
        SystemParametersInfoW(
            SPI_SETDESKWALLPAPER,
            0,
            Some(wide.as_mut_ptr() as *mut c_void),
            SPIF_UPDATEINIFILE | SPIF_SENDWININICHANGE,
        )
    }
    .context("SystemParametersInfoW(SPI_SETDESKWALLPAPER) failed")?;

    Ok(())
}

pub struct WindowsBackend;

impl WindowsBackend {
    pub fn new() -> Self {
        Self
    }
}

impl WallpaperBackend for WindowsBackend {
    fn name(&self) -> String {
        "Windows".to_string()
    }

    fn set(&self, path: &Path, mode: FillMode) -> Result<()> {
        let wallpaper = match interface() {
            Ok(wallpaper) => wallpaper,
            Err(err) => {
                log::warn!("falling back to SystemParametersInfoW: {err:#}");
                return set_via_system_parameters(path);
            }
        };

        let image = HSTRING::from(path.as_os_str());
        unsafe {
            wallpaper
                .SetPosition(position(mode))
                .context("setting the wallpaper fill mode")?;
            // A null monitor id applies the image to every monitor.
            wallpaper
                .SetWallpaper(PCWSTR::null(), &image)
                .context("setting the wallpaper")?;
        }
        Ok(())
    }

    fn set_per_monitor(&self, paths: &[PathBuf], mode: FillMode) -> Result<()> {
        if paths.is_empty() {
            return Ok(());
        }

        let wallpaper = interface()?;
        let monitors = monitor_ids(&wallpaper)?;
        if monitors.is_empty() {
            return self.set(&paths[0], mode);
        }

        unsafe {
            wallpaper
                .SetPosition(position(mode))
                .context("setting the wallpaper fill mode")?;
        }

        for (index, monitor) in monitors.iter().enumerate() {
            let image = HSTRING::from(paths[index % paths.len()].as_os_str());
            unsafe {
                wallpaper
                    .SetWallpaper(monitor, &image)
                    .with_context(|| format!("setting the wallpaper on monitor {index}"))?;
            }
        }
        Ok(())
    }

    fn monitor_count(&self) -> usize {
        interface()
            .and_then(|wallpaper| monitor_ids(&wallpaper))
            .map(|ids| ids.len().max(1))
            .unwrap_or(1)
    }

    fn supports_per_monitor(&self) -> bool {
        true
    }
}
