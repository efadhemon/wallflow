use super::live::{blit_for, file_uri, LiveOpts};
use anyhow::{anyhow, bail, Context, Result};
use std::io::Read;
use std::os::fd::AsFd;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_output, wl_registry, wl_shm, wl_shm_pool, wl_surface,
};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{self, Layer},
    zwlr_layer_surface_v1::{self, Anchor, KeyboardInteractivity},
};

pub fn available() -> bool {
    match connect_app() {
        Ok((mut queue, mut app)) => {
            let _ = queue.roundtrip(&mut app);
            app.layer_shell.is_some() && app.compositor.is_some() && app.shm.is_some()
        }
        Err(_) => false,
    }
}

pub fn run(
    path: &Path,
    opts: &Mutex<LiveOpts>,
    stop: &AtomicBool,
    pause: &AtomicBool,
    child_slot: &Mutex<Option<Child>>,
    ready: std::sync::mpsc::Sender<Result<()>>,
) -> Result<()> {
    if !command_exists("gst-launch-1.0") {
        bail!(
            "GStreamer is not installed. Install gstreamer1.0-plugins-base-apps and gstreamer1.0-libav."
        );
    }

    let mut announced = false;
    while !stop.load(Ordering::SeqCst) {
        if pause.load(Ordering::SeqCst) {
            kill_child(child_slot);
            thread::sleep(Duration::from_millis(120));
            continue;
        }

        match play_once(path, current_opts(opts), stop, pause, child_slot) {
            Ok(()) => {
                if !announced {
                    let _ = ready.send(Ok(()));
                    announced = true;
                }
            }
            Err(err) => {
                if !announced {
                    let _ = ready.send(Err(anyhow!("{err:#}")));
                    return Err(err);
                }
                log::warn!("live wallpaper pipeline restarted: {err:#}");
                thread::sleep(Duration::from_secs(1));
            }
        }
    }
    Ok(())
}

fn current_opts(opts: &Mutex<LiveOpts>) -> LiveOpts {
    opts.lock().map(|guard| *guard).unwrap_or(LiveOpts {
        muted: true,
        fill_mode: crate::config::FillMode::Fill,
        pause_on_fullscreen: true,
        pause_on_battery: false,
    })
}

fn command_exists(name: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {name}"))
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

fn kill_child(child: &Mutex<Option<Child>>) {
    if let Ok(mut slot) = child.lock() {
        if let Some(mut proc) = slot.take() {
            let _ = proc.kill();
            let _ = proc.wait();
        }
    }
}

#[derive(Clone)]
struct OutputSize {
    width: u32,
    height: u32,
}

struct SurfaceSlot {
    surface: wl_surface::WlSurface,
    #[allow(dead_code)]
    layer: zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
    width: u32,
    height: u32,
    configured: bool,
    mmap: memmap2::MmapMut,
    buffer: wl_buffer::WlBuffer,
    #[allow(dead_code)]
    pool: wl_shm_pool::WlShmPool,
    #[allow(dead_code)]
    file: std::fs::File,
}

struct App {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    layer_shell: Option<zwlr_layer_shell_v1::ZwlrLayerShellV1>,
    outputs: Vec<wl_output::WlOutput>,
    output_sizes: Vec<OutputSize>,
    surfaces: Vec<SurfaceSlot>,
}

fn connect_app() -> Result<(wayland_client::EventQueue<App>, App)> {
    let conn = Connection::connect_to_env().context("connecting to Wayland")?;
    let display = conn.display();
    let event_queue = conn.new_event_queue();
    let qh = event_queue.handle();
    let _registry = display.get_registry(&qh, ());
    Ok((
        event_queue,
        App {
            compositor: None,
            shm: None,
            layer_shell: None,
            outputs: Vec::new(),
            output_sizes: Vec::new(),
            surfaces: Vec::new(),
        },
    ))
}

impl Dispatch<wl_registry::WlRegistry, ()> for App {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_compositor" => {
                    state.compositor = Some(registry.bind(name, version.min(4), qh, ()));
                }
                "wl_shm" => {
                    state.shm = Some(registry.bind(name, version.min(1), qh, ()));
                }
                "zwlr_layer_shell_v1" => {
                    state.layer_shell = Some(registry.bind(name, version.min(4), qh, ()));
                }
                "wl_output" => {
                    let index = state.outputs.len();
                    state.output_sizes.push(OutputSize {
                        width: 0,
                        height: 0,
                    });
                    state
                        .outputs
                        .push(registry.bind(name, version.min(3), qh, index));
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_output::WlOutput, usize> for App {
    fn event(
        state: &mut Self,
        _proxy: &wl_output::WlOutput,
        event: wl_output::Event,
        index: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Mode { width, height, .. } = event {
            if let Some(size) = state.output_sizes.get_mut(*index) {
                if width > 0 && height > 0 {
                    size.width = width as u32;
                    size.height = height as u32;
                }
            }
        }
    }
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, usize> for App {
    fn event(
        state: &mut Self,
        proxy: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        index: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_layer_surface_v1::Event::Configure {
            serial,
            width,
            height,
        } = event
        {
            proxy.ack_configure(serial);
            if let Some(slot) = state.surfaces.get_mut(*index) {
                if width > 0 {
                    slot.width = width;
                }
                if height > 0 {
                    slot.height = height;
                }
                slot.configured = true;
            }
        }
    }
}

wayland_client::delegate_noop!(App: ignore wl_compositor::WlCompositor);
wayland_client::delegate_noop!(App: ignore wl_shm::WlShm);
wayland_client::delegate_noop!(App: ignore wl_shm_pool::WlShmPool);
wayland_client::delegate_noop!(App: ignore wl_buffer::WlBuffer);
wayland_client::delegate_noop!(App: ignore wl_surface::WlSurface);
wayland_client::delegate_noop!(App: ignore zwlr_layer_shell_v1::ZwlrLayerShellV1);

fn create_slot(
    app: &App,
    qh: &QueueHandle<App>,
    output: &wl_output::WlOutput,
    index: usize,
    width: u32,
    height: u32,
) -> Result<SurfaceSlot> {
    let compositor = app
        .compositor
        .as_ref()
        .ok_or_else(|| anyhow!("wl_compositor is missing"))?;
    let shm = app
        .shm
        .as_ref()
        .ok_or_else(|| anyhow!("wl_shm is missing"))?;
    let layer_shell = app
        .layer_shell
        .as_ref()
        .ok_or_else(|| anyhow!("zwlr_layer_shell_v1 is missing"))?;

    let width = width.max(1);
    let height = height.max(1);
    let stride = width.saturating_mul(4);
    let size = stride.saturating_mul(height).max(4) as usize;
    let file = tempfile::tempfile().context("creating a shared-memory buffer")?;
    file.set_len(size as u64)?;
    let mmap = unsafe { memmap2::MmapMut::map_mut(&file) }.context("mapping the shm buffer")?;
    let pool = shm.create_pool(file.as_fd(), size as i32, qh, ());
    let buffer = pool.create_buffer(
        0,
        width as i32,
        height as i32,
        stride as i32,
        wl_shm::Format::Argb8888,
        qh,
        (),
    );
    let surface = compositor.create_surface(qh, ());
    let layer = layer_shell.get_layer_surface(
        &surface,
        Some(output),
        Layer::Background,
        "wallflow".to_string(),
        qh,
        index,
    );
    layer.set_anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right);
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.set_size(width, height);
    surface.commit();
    Ok(SurfaceSlot {
        surface,
        layer,
        width,
        height,
        configured: false,
        mmap,
        buffer,
        pool,
        file,
    })
}

fn play_once(
    path: &Path,
    opts: LiveOpts,
    stop: &AtomicBool,
    pause: &AtomicBool,
    child_slot: &Mutex<Option<Child>>,
) -> Result<()> {
    let (mut event_queue, mut app) = connect_app()?;
    event_queue.roundtrip(&mut app)?;
    if app.layer_shell.is_none() {
        bail!("This Wayland compositor does not support wlr-layer-shell background surfaces.");
    }
    event_queue.roundtrip(&mut app)?;

    let qh = event_queue.handle();
    let sizes: Vec<OutputSize> = app
        .output_sizes
        .iter()
        .map(|size| OutputSize {
            width: size.width.max(1),
            height: size.height.max(1),
        })
        .collect();
    if sizes.is_empty() {
        bail!("no Wayland outputs were advertised");
    }

    let outputs = app.outputs.clone();
    for (index, (output, size)) in outputs.iter().zip(&sizes).enumerate() {
        let slot = create_slot(&app, &qh, output, index, size.width, size.height)?;
        app.surfaces.push(slot);
    }
    event_queue.roundtrip(&mut app)?;
    if app.surfaces.iter().any(|slot| !slot.configured) {
        event_queue.roundtrip(&mut app)?;
    }

    let (video_w, video_h) = sizes
        .iter()
        .max_by_key(|size| size.width.saturating_mul(size.height))
        .map(|size| (size.width, size.height))
        .unwrap_or((1920, 1080));

    let mut child = spawn_gst(path, opts.muted, video_w, video_h)?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("GStreamer stdout was not piped"))?;
    *child_slot.lock().expect("child lock") = Some(child);

    let frame_len = video_w as usize * video_h as usize * 4;
    let mut frame = vec![0u8; frame_len];
    while !stop.load(Ordering::SeqCst) && !pause.load(Ordering::SeqCst) {
        if stdout.read_exact(&mut frame).is_err() {
            break;
        }
        for slot in &mut app.surfaces {
            blit_frame(&frame, video_w, video_h, slot, opts);
            slot.surface.attach(Some(&slot.buffer), 0, 0);
            slot.surface
                .damage_buffer(0, 0, slot.width as i32, slot.height as i32);
            slot.surface.commit();
        }
        let _ = event_queue.flush();
        let _ = event_queue.dispatch_pending(&mut app);
    }

    kill_child(child_slot);
    Ok(())
}

fn spawn_gst(path: &Path, muted: bool, width: u32, height: u32) -> Result<Child> {
    let uri = file_uri(path);
    let sink = format!(
        "videoconvert ! videoscale ! video/x-raw,format=BGRA,width={width},height={height} ! fdsink fd=1 sync=true"
    );
    let mut command = Command::new("gst-launch-1.0");
    command.args([
        "-q",
        "playbin",
        &format!("uri={uri}"),
        &format!("video-sink={sink}"),
    ]);
    if muted {
        command.arg("mute=true");
    }
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::null());
    command.spawn().context("starting gst-launch-1.0")
}

fn blit_frame(frame: &[u8], video_w: u32, video_h: u32, slot: &mut SurfaceSlot, opts: LiveOpts) {
    let blit = blit_for(opts.fill_mode, video_w, video_h, slot.width, slot.height);
    slot.mmap.fill(0);
    let dst_stride = slot.width as usize * 4;
    let src_stride = video_w as usize * 4;
    for dy in 0..blit.dst_h.max(0) as u32 {
        let sy = blit.src_y as u32 + dy * blit.src_h.max(1) as u32 / blit.dst_h.max(1) as u32;
        if sy >= video_h {
            continue;
        }
        let y = blit.dst_y as u32 + dy;
        if y >= slot.height {
            continue;
        }
        for dx in 0..blit.dst_w.max(0) as u32 {
            let sx = blit.src_x as u32 + dx * blit.src_w.max(1) as u32 / blit.dst_w.max(1) as u32;
            if sx >= video_w {
                continue;
            }
            let x = blit.dst_x as u32 + dx;
            if x >= slot.width {
                continue;
            }
            let src = (sy as usize * src_stride) + (sx as usize * 4);
            let dst = (y as usize * dst_stride) + (x as usize * 4);
            if src + 4 <= frame.len() && dst + 4 <= slot.mmap.len() {
                slot.mmap[dst..dst + 4].copy_from_slice(&frame[src..src + 4]);
                slot.mmap[dst + 3] = 255;
            }
        }
    }
}
