use super::live::{blit_for, LiveOpts};
use super::windows::{ensure_com, interface, monitor_ids, monitor_rect, wallpaper_worker};
use anyhow::{anyhow, bail, Context, Result};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateSolidBrush, FillRect, GetDC, MapWindowPoints, ReleaseDC, StretchDIBits, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
};
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFMediaType, IMFSample, IMFSourceReader, MFCreateAttributes, MFCreateMediaType,
    MFCreateSourceReaderFromURL, MFMediaType_Video, MFStartup, MFVideoFormat_ARGB32,
    MFVideoFormat_RGB24, MFVideoFormat_RGB32, MFSTARTUP_FULL, MF_MT_DEFAULT_STRIDE,
    MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE,
    MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, MF_SOURCE_READER_ALL_STREAMS,
    MF_SOURCE_READER_CONSTANTS, MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING,
    MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_VERSION,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::UI::Shell::{
    SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, IsWindow,
    PeekMessageW, RegisterClassExW, SetParent, SetWindowPos, TranslateMessage, HWND_BOTTOM, MSG,
    PM_REMOVE, SWP_NOACTIVATE, SWP_SHOWWINDOW, WM_DESTROY, WM_ERASEBKGND, WNDCLASSEXW,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP, WS_VISIBLE,
};

fn reader_stream(id: MF_SOURCE_READER_CONSTANTS) -> u32 {
    id.0 as u32
}

fn packed_size(media_type: &IMFMediaType, key: &windows::core::GUID) -> Result<(u32, u32)> {
    let packed = unsafe { media_type.GetUINT64(key) }.context("reading a media attribute")?;
    Ok(((packed >> 32) as u32, packed as u32))
}

const LIVE_CLASS: PCWSTR = w!("WallflowLiveWallpaper");

pub struct Handle {
    stop: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    opts: Arc<Mutex<LiveOpts>>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Handle {
    pub fn pause(&self, paused: bool) {
        self.pause.store(paused, Ordering::SeqCst);
    }

    pub fn update_opts(&self, opts: LiveOpts) {
        if let Ok(mut slot) = self.opts.lock() {
            *slot = opts;
        }
    }
}

pub fn start(path: &Path, opts: LiveOpts) -> Result<Handle> {
    ensure_mf()?;
    let path = path.to_path_buf();
    let stop = Arc::new(AtomicBool::new(false));
    let pause = Arc::new(AtomicBool::new(false));
    let opts = Arc::new(Mutex::new(opts));
    let stop_t = stop.clone();
    let pause_t = pause.clone();
    let opts_t = opts.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();

    let thread = thread::Builder::new()
        .name("wallflow-live".into())
        .spawn(move || {
            if let Err(err) = run_loop(&path, &stop_t, &pause_t, &opts_t, Some(ready_tx)) {
                log::error!("live wallpaper stopped: {err:#}");
            }
        })
        .context("starting the live wallpaper thread")?;

    match ready_rx.recv_timeout(Duration::from_secs(12)) {
        Ok(Ok(())) => {}
        Ok(Err(err)) => {
            stop.store(true, Ordering::SeqCst);
            let _ = thread.join();
            return Err(err);
        }
        Err(_) => {
            stop.store(true, Ordering::SeqCst);
            let _ = thread.join();
            bail!("timed out starting the live wallpaper");
        }
    }

    Ok(Handle {
        stop,
        pause,
        opts,
        thread: Some(thread),
    })
}

pub fn extract_poster(cache_dir: &Path, source: &Path) -> Result<PathBuf> {
    let target = super::live::poster_path(cache_dir, source)?;
    if target.is_file() {
        return Ok(target);
    }

    ensure_mf()?;
    let reader = open_reader(source)?;
    let (width, height) = video_size(&reader)?;
    let Some(frame) = read_rgb_frame(&reader)? else {
        super::live::write_placeholder_poster(&target)?;
        return Ok(target);
    };

    let mut rgb = Vec::with_capacity(frame.len() / 4 * 3);
    for chunk in frame.chunks_exact(4) {
        rgb.push(chunk[2]);
        rgb.push(chunk[1]);
        rgb.push(chunk[0]);
    }
    super::live::save_rgb_jpeg(&target, width, height, &rgb)?;
    Ok(target)
}

fn ensure_mf() -> Result<()> {
    ensure_com();
    static ONCE: OnceLock<Result<(), String>> = OnceLock::new();
    match ONCE.get_or_init(|| {
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }
            .map_err(|err| format!("MFStartup failed: {err}"))
    }) {
        Ok(()) => Ok(()),
        Err(err) => bail!("{err}"),
    }
}

fn open_reader(path: &Path) -> Result<IMFSourceReader> {
    ensure_com();
    let url = HSTRING::from(path.as_os_str());
    let attrs = reader_attributes()?;
    let reader: IMFSourceReader = unsafe { MFCreateSourceReaderFromURL(&url, &attrs) }
        .with_context(|| format!("opening {}", path.display()))?;

    let stream = reader_stream(MF_SOURCE_READER_FIRST_VIDEO_STREAM);
    unsafe {
        reader
            .SetStreamSelection(reader_stream(MF_SOURCE_READER_ALL_STREAMS), false)
            .context("disabling unused media streams")?;
        reader
            .SetStreamSelection(stream, true)
            .context("selecting the video stream")?;
    }

    let native = unsafe { reader.GetNativeMediaType(stream, 0) }.ok();
    for subtype in [MFVideoFormat_RGB32, MFVideoFormat_ARGB32, MFVideoFormat_RGB24] {
        if let Some(native) = native.as_ref() {
            if let Ok(media_type) = output_type_from_native(native, &subtype) {
                if unsafe { reader.SetCurrentMediaType(stream, None, &media_type) }.is_ok() {
                    return Ok(reader);
                }
            }
        }
        if let Ok(media_type) = output_type_partial(&subtype) {
            if unsafe { reader.SetCurrentMediaType(stream, None, &media_type) }.is_ok() {
                return Ok(reader);
            }
        }
    }

    bail!(
        "Windows could not convert this video to RGB. Use an H.264 MP4, or install HEVC Video Extensions for HEVC files."
    );
}

fn reader_attributes() -> Result<IMFAttributes> {
    let mut attrs = None;
    unsafe { MFCreateAttributes(&mut attrs, 2) }.context("creating Media Foundation attributes")?;
    let attrs = attrs.context("creating Media Foundation attributes")?;
    unsafe {
        let _ = attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1);
        let _ = attrs.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1);
    }
    Ok(attrs)
}

fn output_type_from_native(
    native: &IMFMediaType,
    subtype: &windows::core::GUID,
) -> Result<IMFMediaType> {
    let media_type = output_type_partial(subtype)?;
    if let Ok(size) = unsafe { native.GetUINT64(&MF_MT_FRAME_SIZE) } {
        let _ = unsafe { media_type.SetUINT64(&MF_MT_FRAME_SIZE, size) };
    }
    if let Ok(rate) = unsafe { native.GetUINT64(&MF_MT_FRAME_RATE) } {
        let _ = unsafe { media_type.SetUINT64(&MF_MT_FRAME_RATE, rate) };
    }
    Ok(media_type)
}

fn output_type_partial(subtype: &windows::core::GUID) -> Result<IMFMediaType> {
    let media_type = unsafe { MFCreateMediaType() }.context("creating an RGB media type")?;
    unsafe {
        media_type
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .context("setting MF_MT_MAJOR_TYPE")?;
        media_type
            .SetGUID(&MF_MT_SUBTYPE, subtype)
            .context("setting the RGB subtype")?;
    }
    Ok(media_type)
}

fn video_size(reader: &IMFSourceReader) -> Result<(u32, u32)> {
    let media_type =
        unsafe { reader.GetCurrentMediaType(reader_stream(MF_SOURCE_READER_FIRST_VIDEO_STREAM)) }
            .context("reading the decoded video size")?;
    let (width, height) = packed_size(&media_type, &MF_MT_FRAME_SIZE)?;
    if width == 0 || height == 0 {
        bail!("video frame size is empty");
    }
    Ok((width, height))
}

fn frame_duration(reader: &IMFSourceReader) -> Duration {
    let Ok(media_type) = (unsafe {
        reader.GetCurrentMediaType(reader_stream(MF_SOURCE_READER_FIRST_VIDEO_STREAM))
    }) else {
        return Duration::from_millis(33);
    };
    if let Ok((num, den)) = packed_size(&media_type, &MF_MT_FRAME_RATE) {
        if num > 0 && den > 0 {
            let fps = num as f64 / den as f64;
            if fps > 1.0 {
                return Duration::from_secs_f64(1.0 / fps);
            }
        }
    }
    Duration::from_millis(33)
}

fn read_rgb_frame(reader: &IMFSourceReader) -> Result<Option<Vec<u8>>> {
    let mut flags = 0u32;
    let mut sample: Option<IMFSample> = None;
    unsafe {
        reader
            .ReadSample(
                reader_stream(MF_SOURCE_READER_FIRST_VIDEO_STREAM),
                0,
                None,
                Some(&mut flags),
                None,
                Some(&mut sample),
            )
            .context("ReadSample")?;
    }

    // MF_SOURCE_READERF_ENDOFSTREAM
    if flags & 0x2 != 0 {
        unsafe {
            let _ = reader.SetCurrentPosition(
                &windows::core::GUID::zeroed(),
                &windows::Win32::System::Com::StructuredStorage::PROPVARIANT::default(),
            );
        }
        return Ok(None);
    }

    let Some(sample) = sample else {
        return Ok(None);
    };
    let buffer = unsafe { sample.ConvertToContiguousBuffer() }.context("video buffer")?;
    let mut data = std::ptr::null_mut();
    let mut max_len = 0u32;
    let mut len = 0u32;
    unsafe {
        buffer
            .Lock(&mut data, Some(&mut max_len), Some(&mut len))
            .context("locking the video buffer")?;
    }
    let bytes = unsafe { std::slice::from_raw_parts(data, len as usize) };
    let frame = flatten_bgra(&reader, bytes);
    unsafe {
        let _ = buffer.Unlock();
    }
    Ok(Some(frame))
}

fn flatten_bgra(reader: &IMFSourceReader, src: &[u8]) -> Vec<u8> {
    let Ok(media_type) = (unsafe {
        reader.GetCurrentMediaType(reader_stream(MF_SOURCE_READER_FIRST_VIDEO_STREAM))
    }) else {
        return src.to_vec();
    };
    let Ok((width, height)) = packed_size(&media_type, &MF_MT_FRAME_SIZE) else {
        return src.to_vec();
    };
    let subtype = unsafe { media_type.GetGUID(&MF_MT_SUBTYPE) }.unwrap_or(MFVideoFormat_RGB32);
    let bpp = if subtype == MFVideoFormat_RGB24 { 3 } else { 4 };
    let stride = match unsafe { media_type.GetUINT32(&MF_MT_DEFAULT_STRIDE) } {
        Ok(raw) => (raw as i32).unsigned_abs().max(width * bpp),
        Err(_) => width * bpp,
    };
    let mut out = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        let row = (y * stride) as usize;
        for x in 0..width {
            let i = row + (x * bpp) as usize;
            let o = ((y * width + x) * 4) as usize;
            if i + 2 >= src.len() || o + 3 >= out.len() {
                return out;
            }
            out[o] = src[i];
            out[o + 1] = src[i + 1];
            out[o + 2] = src[i + 2];
            out[o + 3] = 255;
        }
    }
    out
}

unsafe extern "system" fn live_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_ERASEBKGND {
        return LRESULT(1);
    }
    if msg == WM_DESTROY {
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn register_live_class() -> Result<()> {
    static ONCE: std::sync::Once = std::sync::Once::new();
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
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(live_wnd_proc),
            hInstance: instance,
            lpszClassName: LIVE_CLASS,
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

struct Surface {
    hwnd: HWND,
    parent: Option<HWND>,
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

fn surfaces_need_rebuild(surfaces: &[Surface]) -> bool {
    if surfaces.is_empty() {
        return true;
    }
    surfaces.iter().any(|surface| unsafe {
        !IsWindow(Some(surface.hwnd)).as_bool()
            || surface
                .parent
                .is_some_and(|parent| !IsWindow(Some(parent)).as_bool())
    })
}

fn create_surfaces() -> Result<Vec<Surface>> {
    register_live_class()?;
    let wallpaper = interface()?;
    let monitors = monitor_ids(&wallpaper).unwrap_or_default();
    if monitors.is_empty() {
        bail!("no monitors available");
    }
    let parent = wallpaper_worker();
    let instance = unsafe { GetModuleHandleW(None) }.ok().map(Into::into);
    let style = WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT;
    let mut surfaces = Vec::new();

    for monitor in &monitors {
        let rect = monitor_rect(&wallpaper, monitor)?;
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        if width <= 0 || height <= 0 {
            continue;
        }

        let hwnd = unsafe {
            CreateWindowExW(
                style,
                LIVE_CLASS,
                PCWSTR::null(),
                WS_POPUP | WS_VISIBLE,
                rect.left,
                rect.top,
                width,
                height,
                None,
                None,
                instance,
                None,
            )
        }
        .context("creating the live wallpaper window")?;

        let mut x = rect.left;
        let mut y = rect.top;
        if let Some(parent) = parent {
            let mut point = windows::Win32::Foundation::POINT { x, y };
            unsafe {
                MapWindowPoints(None, Some(parent), std::slice::from_mut(&mut point));
                let _ = SetParent(hwnd, Some(parent));
            }
            x = point.x;
            y = point.y;
        }

        unsafe {
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_BOTTOM),
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }

        surfaces.push(Surface { hwnd, parent });
    }

    if surfaces.is_empty() {
        bail!("could not create a live wallpaper window");
    }
    Ok(surfaces)
}

fn paint_frame(surface: &Surface, bgra: &[u8], video_w: u32, video_h: u32, opts: LiveOpts) {
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(surface.hwnd, &mut client);
    }
    let win_w = (client.right - client.left).max(1) as u32;
    let win_h = (client.bottom - client.top).max(1) as u32;
    let blit = blit_for(opts.fill_mode, video_w, video_h, win_w, win_h);

    let hdc = unsafe { GetDC(Some(surface.hwnd)) };
    if hdc.is_invalid() {
        return;
    }

    unsafe {
        let brush = CreateSolidBrush(COLORREF(0));
        let _ = FillRect(hdc, &client, brush);
        let _ = windows::Win32::Graphics::Gdi::DeleteObject(brush.into());
    }

    let header = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: video_w as i32,
        biHeight: -(video_h as i32),
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let info = BITMAPINFO {
        bmiHeader: header,
        ..Default::default()
    };

    unsafe {
        StretchDIBits(
            hdc,
            blit.dst_x,
            blit.dst_y,
            blit.dst_w,
            blit.dst_h,
            blit.src_x,
            blit.src_y,
            blit.src_w,
            blit.src_h,
            Some(bgra.as_ptr().cast()),
            &info,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
        let _ = ReleaseDC(Some(surface.hwnd), hdc);
    }
}

fn pump() {
    let mut msg = MSG::default();
    while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn system_should_pause(opts: LiveOpts) -> bool {
    if opts.pause_on_fullscreen {
        if let Ok(state) = unsafe { SHQueryUserNotificationState() } {
            if state == QUNS_RUNNING_D3D_FULL_SCREEN
                || state == QUNS_BUSY
                || state == QUNS_PRESENTATION_MODE
            {
                return true;
            }
        }
    }
    if opts.pause_on_battery {
        let mut status = SYSTEM_POWER_STATUS::default();
        if unsafe { GetSystemPowerStatus(&mut status) }.is_ok() && status.ACLineStatus == 0 {
            return true;
        }
    }
    false
}

fn run_loop(
    path: &Path,
    stop: &AtomicBool,
    pause: &AtomicBool,
    opts: &Mutex<LiveOpts>,
    ready: Option<std::sync::mpsc::Sender<Result<()>>>,
) -> Result<()> {
    ensure_com();
    let send_ready = |result: Result<()>| {
        if let Some(tx) = &ready {
            let _ = tx.send(result);
        }
    };

    let reader = match open_reader(path) {
        Ok(reader) => reader,
        Err(err) => {
            send_ready(Err(anyhow!("{err:#}")));
            return Err(err);
        }
    };
    let (video_w, video_h) = match video_size(&reader) {
        Ok(size) => size,
        Err(err) => {
            send_ready(Err(anyhow!("{err:#}")));
            return Err(err);
        }
    };
    let frame_time = frame_duration(&reader);
    let mut surfaces = match create_surfaces() {
        Ok(surfaces) => surfaces,
        Err(err) => {
            send_ready(Err(anyhow!("{err:#}")));
            return Err(err);
        }
    };

    let mut announced = false;
    while !stop.load(Ordering::SeqCst) {
        pump();
        if surfaces_need_rebuild(&surfaces) {
            drop(std::mem::take(&mut surfaces));
            match create_surfaces() {
                Ok(next) => surfaces = next,
                Err(err) => {
                    log::warn!("recreating the live wallpaper surface failed: {err:#}");
                    thread::sleep(Duration::from_millis(400));
                    continue;
                }
            }
        }
        let current = opts.lock().map(|guard| *guard).unwrap_or(LiveOpts {
            muted: true,
            fill_mode: crate::config::FillMode::Fill,
            pause_on_fullscreen: true,
            pause_on_battery: false,
        });
        let held = pause.load(Ordering::SeqCst) || system_should_pause(current);
        if held {
            thread::sleep(Duration::from_millis(80));
            continue;
        }

        let started = Instant::now();
        match read_rgb_frame(&reader) {
            Ok(Some(frame)) => {
                if frame.len() >= (video_w * video_h * 4) as usize {
                    for surface in &surfaces {
                        paint_frame(surface, &frame, video_w, video_h, current);
                    }
                    if !announced {
                        send_ready(Ok(()));
                        announced = true;
                    }
                }
            }
            Ok(None) => {}
            Err(err) => {
                if !announced {
                    send_ready(Err(anyhow!("{err:#}")));
                    return Err(err);
                }
                log::warn!("live wallpaper frame failed: {err:#}");
            }
        }

        let rest = frame_time.saturating_sub(started.elapsed());
        if !rest.is_zero() {
            thread::sleep(rest);
        }
    }
    Ok(())
}
