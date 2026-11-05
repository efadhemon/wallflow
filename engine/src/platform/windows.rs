//! Windows: the video windows become children of Explorer's desktop, under the
//! icons, and pause while a maximized or full-screen window covers them.
//!
//! Explorer builds the desktop in one of two ways, so the layout is read from
//! the window tree each time, not from the Windows version:
//!
//! - Classic (Windows 10, Windows 11 up to 23H2): after message `0x052C`,
//!   Progman splits the desktop into a top-level `WorkerW` holding the icons
//!   (`SHELLDLL_DefView`) and a second top-level `WorkerW` behind it, the
//!   wallpaper layer. The video goes into that one.
//! - Raised (Windows 11 24H2 and later): Progman has `WS_EX_NOREDIRECTIONBITMAP`,
//!   the icons and the wallpaper `WorkerW` are its children, and a plain child
//!   window there is never composited. The video becomes a layered child of
//!   Progman (constant alpha, so presents stay hardware accelerated), right
//!   under the icons and above Explorer's own wallpaper.
//!
//! Both are Explorer internals that changed before, so every decision is logged.

use super::Rect;
use anyhow::{Context, Result, bail};
use std::ffi::c_void;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    HMONITOR, MONITOR_DEFAULTTONEAREST, MonitorFromRect, MonitorFromWindow, ScreenToClient,
};
use windows::Win32::System::StationsAndDesktops::{
    CloseDesktop, DESKTOP_CONTROL_FLAGS, DESKTOP_SWITCHDESKTOP, OpenInputDesktop, SwitchDesktop,
};
use windows::Win32::UI::Shell::{
    QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, FindWindowW, GW_HWNDNEXT, GWL_EXSTYLE, GWL_STYLE, GetClassNameW,
    GetParent, GetWindow, GetWindowLongPtrW, GetWindowRect, HWND_TOP, IsIconic, IsWindow,
    IsWindowVisible, IsZoomed, LWA_ALPHA, SMTO_NORMAL, SWP_FRAMECHANGED, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SendMessageTimeoutW, SetLayeredWindowAttributes,
    SetParent, SetWindowLongPtrW, SetWindowPos, WS_CHILD, WS_EX_LAYERED, WS_EX_NOREDIRECTIONBITMAP,
    WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{BOOL, PCWSTR, w};
use winit::platform::windows::WindowAttributesExtWindows;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{Window, WindowAttributes};

/// Asks Progman to create the wallpaper layer (undocumented, used by every
/// live wallpaper app).
const SPAWN_WORKERW: u32 = 0x052C;

/// Explorer also keeps tiny helper `WorkerW` windows; the wallpaper one spans
/// the desktop.
const MIN_LAYER_SIZE: i32 = 200;

pub fn background_attributes(attrs: WindowAttributes) -> WindowAttributes {
    attrs.with_skip_taskbar(true)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Desktop {
    /// The window the video windows become children of.
    parent: HWND,
    /// Raised layout: the icon view, which the video sits directly under.
    icons: Option<HWND>,
}

pub fn attach(window: &Window, rect: Rect) -> Result<()> {
    let hwnd = hwnd(window)?;
    let desktop = match find_desktop() {
        Some(desktop) => desktop,
        None => {
            let progman = progman().context("Explorer's desktop (Progman) is not running")?;
            log::info!("asking Explorer to create the wallpaper layer");
            unsafe {
                SendMessageTimeoutW(
                    progman,
                    SPAWN_WORKERW,
                    WPARAM(0xD),
                    LPARAM(1),
                    SMTO_NORMAL,
                    1000,
                    None,
                );
            }
            find_desktop().context("could not find the desktop's wallpaper layer")?
        }
    };
    place(hwnd, desktop, rect)
}

/// Every second: put windows back if Explorer rebuilt its desktop (a new
/// wallpaper, a theme change), then work out which are covered.
pub fn check(windows: &[(&Window, Rect)]) -> Vec<bool> {
    let desktop = find_desktop();
    for (window, rect) in windows {
        let (Ok(hwnd), Some(desktop)) = (hwnd(window), desktop) else {
            continue;
        };
        if let Err(err) = keep_placed(hwnd, desktop, *rect) {
            log::warn!("could not keep the wallpaper window behind the desktop: {err:#}");
        }
    }
    covered(windows.iter().map(|(_, rect)| *rect).collect())
}

fn hwnd(window: &Window) -> Result<HWND> {
    match window.window_handle()?.as_raw() {
        RawWindowHandle::Win32(handle) => Ok(HWND(handle.hwnd.get() as *mut c_void)),
        other => bail!("not a Win32 window: {other:?}"),
    }
}

fn progman() -> Option<HWND> {
    unsafe { FindWindowW(w!("Progman"), PCWSTR::null()) }.ok()
}

fn child(parent: Option<HWND>, after: Option<HWND>, class: PCWSTR) -> Option<HWND> {
    unsafe { FindWindowExW(parent, after, class, PCWSTR::null()) }.ok()
}

fn size(hwnd: HWND) -> (i32, i32) {
    let mut rect = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut rect) }.is_err() {
        return (0, 0);
    }
    (rect.right - rect.left, rect.bottom - rect.top)
}

/// The wallpaper layer as Explorer has it now, or `None` before it exists.
fn find_desktop() -> Option<Desktop> {
    let progman = progman()?;
    let ex_style = unsafe { GetWindowLongPtrW(progman, GWL_EXSTYLE) };
    let no_redirection = ex_style & WS_EX_NOREDIRECTIONBITMAP.0 as isize != 0;
    let icons = child(Some(progman), None, w!("SHELLDLL_DefView"));
    let layer_child = child(Some(progman), None, w!("WorkerW")).filter(|&worker| {
        let (width, height) = size(worker);
        width >= MIN_LAYER_SIZE && height >= MIN_LAYER_SIZE
    });

    // Raised wins: a stale top-level WorkerW can still exist there, but
    // windows put into it are never shown.
    if let Some(icons) = icons
        && (no_redirection || layer_child.is_some())
    {
        return Some(Desktop {
            parent: progman,
            icons: Some(icons),
        });
    }
    classic_layer().map(|layer| Desktop {
        parent: layer,
        icons: None,
    })
}

/// Classic layout: the top-level `WorkerW` right after the one holding the icons.
fn classic_layer() -> Option<HWND> {
    unsafe extern "system" fn visit(top: HWND, found: LPARAM) -> BOOL {
        if child(Some(top), None, w!("SHELLDLL_DefView")).is_some()
            && let Some(layer) = child(None, Some(top), w!("WorkerW"))
        {
            unsafe { *(found.0 as *mut Option<HWND>) = Some(layer) };
            return BOOL(0);
        }
        BOOL(1)
    }
    let mut found: Option<HWND> = None;
    // Stopping early reports an "error"; `found` says whether it worked.
    let _ = unsafe { EnumWindows(Some(visit), LPARAM(&mut found as *mut _ as isize)) };
    found
}

fn place(hwnd: HWND, desktop: Desktop, rect: Rect) -> Result<()> {
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        SetWindowLongPtrW(
            hwnd,
            GWL_STYLE,
            (style & !(WS_POPUP.0 as isize)) | WS_CHILD.0 as isize,
        );
        if desktop.icons.is_some() {
            let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style | WS_EX_LAYERED.0 as isize);
            SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA)
                .context("making the window layered")?;
        }
        SetParent(hwnd, Some(desktop.parent)).context("moving the window into the desktop")?;

        // Child windows are placed relative to their parent's client area.
        let mut origin = POINT {
            x: rect.x,
            y: rect.y,
        };
        let _ = ScreenToClient(desktop.parent, &mut origin);
        SetWindowPos(
            hwnd,
            Some(desktop.icons.unwrap_or(HWND_TOP)),
            origin.x,
            origin.y,
            rect.width as i32,
            rect.height as i32,
            SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_FRAMECHANGED,
        )
        .context("placing the window")?;
    }
    log::info!(
        "wallpaper window at {},{} {}x{} in the {} desktop",
        rect.x,
        rect.y,
        rect.width,
        rect.height,
        if desktop.icons.is_some() {
            "raised (24H2+)"
        } else {
            "classic"
        }
    );
    Ok(())
}

fn keep_placed(hwnd: HWND, desktop: Desktop, rect: Rect) -> Result<()> {
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return Ok(());
    }
    let parent = unsafe { GetParent(hwnd) }.ok();
    if parent != Some(desktop.parent) {
        log::info!("Explorer rebuilt the desktop, placing the wallpaper window again");
        return place(hwnd, desktop, rect);
    }
    // Raised: Explorer can put its own wallpaper back on top of ours.
    if let Some(icons) = desktop.icons
        && !above_wallpaper(icons, hwnd)
    {
        unsafe {
            SetWindowPos(
                hwnd,
                Some(icons),
                0,
                0,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
            )
        }
        .context("putting the window back under the icons")?;
    }
    Ok(())
}

/// Raised layout: whether `hwnd` is still between the icons and Explorer's own
/// wallpaper `WorkerW` (with several monitors, the other video windows are
/// there too, in any order).
fn above_wallpaper(icons: HWND, hwnd: HWND) -> bool {
    let mut next = unsafe { GetWindow(icons, GW_HWNDNEXT) }.ok();
    while let Some(sibling) = next {
        if sibling == hwnd {
            return true;
        }
        if class_name(sibling) == "WorkerW" {
            return false;
        }
        next = unsafe { GetWindow(sibling, GW_HWNDNEXT) }.ok();
    }
    false
}

fn class_name(hwnd: HWND) -> String {
    let mut class = [0u16; 64];
    let len = unsafe { GetClassNameW(hwnd, &mut class) } as usize;
    String::from_utf16_lossy(&class[..len.min(class.len())])
}

/// The lock screen, a full-screen Direct3D game or presentation mode hide
/// every monitor.
fn desktop_hidden() -> bool {
    let state = unsafe { SHQueryUserNotificationState() };
    if let Ok(state) = state
        && (state == QUNS_RUNNING_D3D_FULL_SCREEN || state == QUNS_PRESENTATION_MODE)
    {
        return true;
    }
    // Locked (or on the secure desktop): the input desktop cannot be switched to.
    match unsafe { OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_SWITCHDESKTOP) } {
        Ok(desktop) => {
            let locked = unsafe { SwitchDesktop(desktop) }.is_err();
            let _ = unsafe { CloseDesktop(desktop) };
            locked
        }
        Err(_) => true,
    }
}

struct Scan {
    screens: Vec<Rect>,
    monitors: Vec<HMONITOR>,
    covered: Vec<bool>,
}

fn covered(screens: Vec<Rect>) -> Vec<bool> {
    if desktop_hidden() {
        return vec![true; screens.len()];
    }
    let monitors = screens
        .iter()
        .map(|rect| {
            let area = RECT {
                left: rect.x,
                top: rect.y,
                right: rect.x + rect.width as i32,
                bottom: rect.y + rect.height as i32,
            };
            unsafe { MonitorFromRect(&area, MONITOR_DEFAULTTONEAREST) }
        })
        .collect();
    let mut scan = Scan {
        covered: vec![false; screens.len()],
        screens,
        monitors,
    };

    unsafe extern "system" fn visit(top: HWND, scan: LPARAM) -> BOOL {
        let scan = unsafe { &mut *(scan.0 as *mut Scan) };
        if let Some(area) = covering_area(top) {
            let zoomed = unsafe { IsZoomed(top) }.as_bool();
            let monitor = unsafe { MonitorFromWindow(top, MONITOR_DEFAULTTONEAREST) };
            for (i, screen) in scan.screens.iter().enumerate() {
                if (zoomed && monitor == scan.monitors[i]) || area.contains(screen) {
                    scan.covered[i] = true;
                }
            }
        }
        BOOL(1)
    }
    let _ = unsafe { EnumWindows(Some(visit), LPARAM(&mut scan as *mut _ as isize)) };
    scan.covered
}

/// A top-level window that can hide the desktop: shown, not minimized, not on
/// another virtual desktop (cloaked), not part of the shell. Returns its area.
fn covering_area(top: HWND) -> Option<Rect> {
    unsafe {
        if !IsWindowVisible(top).as_bool() || IsIconic(top).as_bool() {
            return None;
        }
        // Tool windows, and click-through overlays (GPU or recording overlays
        // can span the whole screen without hiding anything).
        let ex_style = GetWindowLongPtrW(top, GWL_EXSTYLE);
        if ex_style & (WS_EX_TOOLWINDOW.0 | WS_EX_TRANSPARENT.0) as isize != 0 {
            return None;
        }
        let mut cloaked = 0u32;
        if DwmGetWindowAttribute(
            top,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as *mut c_void,
            size_of::<u32>() as u32,
        )
        .is_ok()
            && cloaked != 0
        {
            return None;
        }
        if matches!(
            class_name(top).as_str(),
            "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
        ) {
            return None;
        }
        // The visible frame, without the invisible resize borders.
        let mut area = RECT::default();
        DwmGetWindowAttribute(
            top,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut area as *mut RECT as *mut c_void,
            size_of::<RECT>() as u32,
        )
        .ok()?;
        if area.right <= area.left || area.bottom <= area.top {
            return None;
        }
        Some(Rect {
            x: area.left,
            y: area.top,
            width: (area.right - area.left) as u32,
            height: (area.bottom - area.top) as u32,
        })
    }
}
