use super::live::{LiveOpts, LiveSupport};
use super::render::{compose, ease_in_out_cubic, FADE_FRAME_MS, FADE_MS};
use super::WallpaperBackend;
use crate::config::FillMode;
use anyhow::{bail, Context, Result};
use image::RgbaImage;
use std::ffi::c_void;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, Once};
use std::thread;
use std::time::{Duration, Instant};

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, MapWindowPoints, SelectObject,
    AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC,
    HGDIOBJ,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    DesktopWallpaper, IDesktopWallpaper, DESKTOP_WALLPAPER_POSITION, DWPOS_CENTER, DWPOS_FILL,
    DWPOS_FIT, DWPOS_STRETCH, DWPOS_TILE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EnumWindows, FindWindowExW,
    FindWindowW, PeekMessageW, RegisterClassExW, SendMessageTimeoutW, SetParent, SetWindowPos,
    SystemParametersInfoW, TranslateMessage, UpdateLayeredWindow, HWND_BOTTOM, MSG, PM_REMOVE,
    SMTO_NORMAL, SPIF_SENDWININICHANGE, SPIF_UPDATEINIFILE, SPI_SETDESKWALLPAPER, SWP_NOACTIVATE,
    SWP_SHOWWINDOW, ULW_ALPHA, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TRANSPARENT, WS_POPUP,
};

thread_local! {
    static COM_READY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

thread_local! {
    static COM_READY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// COM must be initialised on every thread that touches it, and the scheduler
/// runs on whichever tokio worker picks up the task.
pub(super) fn ensure_com() {
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

pub(super) fn interface() -> Result<IDesktopWallpaper> {
    ensure_com();
    let wallpaper: IDesktopWallpaper =
        unsafe { CoCreateInstance(&DesktopWallpaper, None, CLSCTX_ALL) }
            .context("creating the IDesktopWallpaper COM object")?;
    Ok(wallpaper)
}

pub(super) fn monitor_ids(wallpaper: &IDesktopWallpaper) -> Result<Vec<HSTRING>> {
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

pub(super) fn monitor_rect(wallpaper: &IDesktopWallpaper, id: &HSTRING) -> Result<RECT> {
    unsafe { wallpaper.GetMonitorRECT(id) }
        .with_context(|| format!("reading the rectangle of monitor {id}"))
}

fn letterbox_of(wallpaper: &IDesktopWallpaper) -> [u8; 4] {
    match unsafe { wallpaper.GetBackgroundColor() } {
        Ok(COLORREF(value)) => [
            (value & 0xFF) as u8,
            ((value >> 8) & 0xFF) as u8,
            ((value >> 16) & 0xFF) as u8,
            255,
        ],
        Err(_) => [0, 0, 0, 255],
    }
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

fn ensure_position(wallpaper: &IDesktopWallpaper, mode: FillMode) -> Result<()> {
    let desired = position(mode);
    let current = unsafe { wallpaper.GetPosition() }.ok();
    if current != Some(desired) {
        unsafe {
            wallpaper
                .SetPosition(desired)
                .context("setting the wallpaper fill mode")?;
        }
    }
    Ok(())
}

pub(super) fn commit_all(wallpaper: &IDesktopWallpaper, path: &Path, mode: FillMode) -> Result<()> {
    ensure_position(wallpaper, mode)?;
    let image = HSTRING::from(path.as_os_str());
    unsafe {
        wallpaper
            .SetWallpaper(PCWSTR::null(), &image)
            .context("setting the wallpaper")?;
    }
    Ok(())
}

fn commit_per_monitor(
    wallpaper: &IDesktopWallpaper,
    monitors: &[HSTRING],
    paths: &[PathBuf],
    mode: FillMode,
) -> Result<()> {
    ensure_position(wallpaper, mode)?;
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

const OVERLAY_CLASS: PCWSTR = w!("WallflowFadeOverlay");

unsafe extern "system" fn overlay_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn register_overlay_class() -> Result<()> {
    static ONCE: Once = Once::new();
    let mut error = None;
    ONCE.call_once(|| {
        let instance = match unsafe { GetModuleHandleW(None) } {
            Ok(module) => module.into(),
            Err(err) => {
                error = Some(err);
                return;
            }
        };
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(overlay_wnd_proc),
            hInstance: instance,
            lpszClassName: OVERLAY_CLASS,
            ..Default::default()
        };
        if unsafe { RegisterClassExW(&class) } == 0 {
            error = Some(windows::core::Error::from_thread());
        }
    });
    if let Some(err) = error {
        return Err(err.into());
    }
    Ok(())
}

unsafe extern "system" fn find_worker(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
    let def_view =
        unsafe { FindWindowExW(Some(hwnd), None, w!("SHELLDLL_DefView"), PCWSTR::null()) };
    if def_view.is_err() {
        return true.into();
    }

    let worker = unsafe { FindWindowExW(Some(hwnd), None, w!("WorkerW"), PCWSTR::null()) }
        .ok()
        .or_else(|| unsafe { FindWindowExW(None, Some(hwnd), w!("WorkerW"), PCWSTR::null()) }.ok());

    if let Some(worker) = worker {
        unsafe { *(lparam.0 as *mut HWND) = worker };
        return false.into();
    }
    true.into()
}

pub(super) fn wallpaper_worker() -> Option<HWND> {
    let progman = unsafe { FindWindowW(w!("Progman"), None) }.ok()?;
    unsafe {
        SendMessageTimeoutW(
            progman,
            0x052C,
            WPARAM(0),
            LPARAM(0),
            SMTO_NORMAL,
            1000,
            None,
        );
    }

    let mut worker = HWND::default();
    let _ = unsafe { EnumWindows(Some(find_worker), LPARAM(&mut worker as *mut HWND as isize)) };
    if worker.is_invalid() {
        None
    } else {
        Some(worker)
    }
}

struct Layer {
    hwnd: HWND,
    hdc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    width: i32,
    height: i32,
}

impl Layer {
    fn paint(&self, alpha: u8) -> Result<()> {
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: alpha,
            AlphaFormat: 0,
        };
        let size = SIZE {
            cx: self.width,
            cy: self.height,
        };
        let src = POINT { x: 0, y: 0 };
        unsafe {
            UpdateLayeredWindow(
                self.hwnd,
                None,
                None,
                Some(std::ptr::from_ref(&size)),
                Some(self.hdc),
                Some(std::ptr::from_ref(&src)),
                COLORREF(0),
                Some(std::ptr::from_ref(&blend)),
                ULW_ALPHA,
            )
            .context("UpdateLayeredWindow")?;
        }
        Ok(())
    }
}

struct FadeOverlay {
    layers: Vec<Layer>,
}

impl Drop for FadeOverlay {
    fn drop(&mut self) {
        for layer in &self.layers {
            unsafe {
                let _ = DestroyWindow(layer.hwnd);
                SelectObject(layer.hdc, layer.previous);
                let _ = DeleteObject(layer.bitmap.into());
                let _ = DeleteDC(layer.hdc);
            }
        }
    }
}

fn copy_bgra(image: &RgbaImage, bits: *mut u8) {
    let width = image.width() as usize;
    let height = image.height() as usize;
    let src = image.as_raw();
    for y in 0..height {
        let src_row = y * width * 4;
        let dst_row = (height - 1 - y) * width * 4;
        for x in 0..width {
            let s = src_row + x * 4;
            let d = dst_row + x * 4;
            unsafe {
                *bits.add(d) = src[s + 2];
                *bits.add(d + 1) = src[s + 1];
                *bits.add(d + 2) = src[s];
                *bits.add(d + 3) = src[s + 3];
            }
        }
    }
}

fn create_layer(rect: RECT, image: &RgbaImage, parent: Option<HWND>) -> Result<Layer> {
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 {
        bail!("monitor rectangle is empty");
    }

    let hdc = unsafe { CreateCompatibleDC(None) };
    if hdc.is_invalid() {
        bail!("CreateCompatibleDC failed");
    }

    let header = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        biHeight: height,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let info = BITMAPINFO {
        bmiHeader: header,
        ..Default::default()
    };

    let mut bits = std::ptr::null_mut();
    let bitmap =
        match unsafe { CreateDIBSection(Some(hdc), &info, DIB_RGB_COLORS, &mut bits, None, 0) } {
            Ok(bitmap) => bitmap,
            Err(err) => {
                unsafe {
                    let _ = DeleteDC(hdc);
                }
                return Err(err.into());
            }
        };
    if bits.is_null() {
        unsafe {
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(hdc);
        }
        bail!("CreateDIBSection returned no bits");
    }
    copy_bgra(image, bits as *mut u8);
    let previous = unsafe { SelectObject(hdc, bitmap.into()) };

    let style = WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW;
    let instance = unsafe { GetModuleHandleW(None) }.ok().map(Into::into);
    let hwnd = match unsafe {
        CreateWindowExW(
            style,
            OVERLAY_CLASS,
            PCWSTR::null(),
            WS_POPUP,
            rect.left,
            rect.top,
            width,
            height,
            None,
            None,
            instance,
            None,
        )
    } {
        Ok(hwnd) => hwnd,
        Err(err) => {
            unsafe {
                SelectObject(hdc, previous);
                let _ = DeleteObject(bitmap.into());
                let _ = DeleteDC(hdc);
            }
            return Err(err.into());
        }
    };

    let mut x = rect.left;
    let mut y = rect.top;
    let insert_after = if let Some(parent) = parent {
        let mut point = POINT { x, y };
        unsafe {
            MapWindowPoints(None, Some(parent), std::slice::from_mut(&mut point));
            let _ = SetParent(hwnd, Some(parent));
        }
        x = point.x;
        y = point.y;
        None
    } else {
        Some(HWND_BOTTOM)
    };

    unsafe {
        let _ = SetWindowPos(
            hwnd,
            insert_after,
            x,
            y,
            width,
            height,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }

    let layer = Layer {
        hwnd,
        hdc,
        bitmap,
        previous,
        width,
        height,
    };
    layer.paint(0)?;
    Ok(layer)
}

fn pump_messages() {
    let mut msg = MSG::default();
    while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn fade_in(layers: &[Layer]) -> Result<()> {
    let start = Instant::now();
    loop {
        let t = (start.elapsed().as_millis() as f32 / FADE_MS as f32).min(1.0);
        let alpha = (ease_in_out_cubic(t) * 255.0).round() as u8;
        for layer in layers {
            layer.paint(alpha)?;
        }
        pump_messages();
        if t >= 1.0 {
            break;
        }
        thread::sleep(Duration::from_millis(FADE_FRAME_MS as u64));
    }
    Ok(())
}

fn fade_overlay(
    wallpaper: &IDesktopWallpaper,
    monitors: &[HSTRING],
    paths: &[PathBuf],
    per_monitor: bool,
    mode: FillMode,
) -> Result<FadeOverlay> {
    register_overlay_class()?;
    let letterbox = letterbox_of(wallpaper);
    let parent = wallpaper_worker();
    let mut layers = Vec::with_capacity(monitors.len());

    for (index, monitor) in monitors.iter().enumerate() {
        let path = if per_monitor {
            &paths[index % paths.len()]
        } else {
            &paths[0]
        };
        let rect = monitor_rect(wallpaper, monitor)?;
        let width = (rect.right - rect.left).max(0) as u32;
        let height = (rect.bottom - rect.top).max(0) as u32;
        let frame = compose(path, mode, width, height, letterbox)?;
        layers.push(create_layer(rect, &frame, parent)?);
    }

    fade_in(&layers)?;
    Ok(FadeOverlay { layers })
}

fn apply(
    wallpaper: &IDesktopWallpaper,
    paths: &[PathBuf],
    mode: FillMode,
    per_monitor: bool,
) -> Result<()> {
    let monitors = monitor_ids(wallpaper).unwrap_or_default();
    let overlay = if monitors.is_empty() {
        None
    } else {
        match fade_overlay(wallpaper, &monitors, paths, per_monitor, mode) {
            Ok(overlay) => Some(overlay),
            Err(err) => {
                log::warn!("wallpaper fade overlay failed: {err:#}");
                None
            }
        }
    };

    let result = if per_monitor && !monitors.is_empty() {
        commit_per_monitor(wallpaper, &monitors, paths, mode)
    } else {
        commit_all(wallpaper, &paths[0], mode)
    };
    drop(overlay);
    result
}

pub struct WindowsBackend {
    live: Mutex<Option<super::windows_live::Handle>>,
}

impl WindowsBackend {
    pub fn new() -> Self {
        Self {
            live: Mutex::new(None),
        }
    }
}

impl WallpaperBackend for WindowsBackend {
    fn name(&self) -> String {
        "Windows".to_string()
    }

    fn set(&self, path: &Path, mode: FillMode) -> Result<()> {
        self.stop_live();
        let wallpaper = match interface() {
            Ok(wallpaper) => wallpaper,
            Err(err) => {
                log::warn!("falling back to SystemParametersInfoW: {err:#}");
                return set_via_system_parameters(path);
            }
        };
        apply(&wallpaper, &[path.to_path_buf()], mode, false)
    }

    fn set_per_monitor(&self, paths: &[PathBuf], mode: FillMode) -> Result<()> {
        self.stop_live();
        if paths.is_empty() {
            return Ok(());
        }

        let wallpaper = match interface() {
            Ok(wallpaper) => wallpaper,
            Err(err) => {
                log::warn!("falling back to SystemParametersInfoW: {err:#}");
                return set_via_system_parameters(&paths[0]);
            }
        };
        apply(&wallpaper, paths, mode, true)
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

    fn live_support(&self) -> LiveSupport {
        LiveSupport::Full
    }

    fn play_live(&self, path: &Path, opts: LiveOpts) -> Result<()> {
        self.stop_live();
        let handle = super::windows_live::start(path, opts)?;
        *self.live.lock().expect("live lock") = Some(handle);
        Ok(())
    }

    fn stop_live(&self) {
        drop(self.live.lock().expect("live lock").take());
    }

    fn pause_live(&self, paused: bool) {
        if let Some(handle) = self.live.lock().expect("live lock").as_ref() {
            handle.pause(paused);
        }
    }

    fn update_live_opts(&self, opts: LiveOpts) {
        if let Some(handle) = self.live.lock().expect("live lock").as_ref() {
            handle.update_opts(opts);
        }
    }

    fn extract_poster(&self, cache_dir: &Path, source: &Path) -> Result<PathBuf> {
        super::windows_live::extract_poster(cache_dir, source)
    }
}
