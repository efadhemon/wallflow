//! Wallflow's live wallpaper engine: decodes a looping video and draws it with the
//! GPU into a borderless window that a small per-desktop shim puts behind the icons.
//!
//! It runs as its own process so a stuck decoder or a GPU reset can never freeze
//! the desktop shell or the Wallflow window. The app drives it over stdin with one
//! JSON command per line, and it exits when stdin closes, so it never outlives the
//! app.

mod decoder;
mod encoder;
mod events;
mod fit;
mod prepare;
mod probe;
mod render;

use anyhow::{Context, Result, bail};
use decoder::{Decoder, Frame};
use events::Event;
use fit::FillMode;
use probe::VideoInfo;
use render::{Gpu, Target};
use serde::Deserialize;
use std::io::BufRead;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

/// Window class / Wayland app id the desktop shims look for.
pub const APP_ID: &str = "wallflow-live";

/// Falling further behind than this (after a pause, a suspend, or the desktop
/// being covered) restarts the clock instead of racing through old frames.
const MAX_LAG: Duration = Duration::from_millis(250);

/// Decoder restarts allowed before the engine gives up and exits.
const MAX_DECODER_RESTARTS: u32 = 5;

/// How often to log playback statistics.
const STATS_EVERY: Duration = Duration::from_secs(10);

struct Opts {
    video: PathBuf,
    fill: FillMode,
    hwaccel: String,
    ffmpeg: String,
    ffprobe: String,
    background: bool,
    events: bool,
}

fn parse_args() -> Result<Opts> {
    let mut args = std::env::args().skip(1);
    let mut video = None;
    let mut opts = Opts {
        video: PathBuf::new(),
        fill: FillMode::Fill,
        hwaccel: "auto".into(),
        ffmpeg: "ffmpeg".into(),
        ffprobe: "ffprobe".into(),
        background: false,
        events: false,
    };
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().with_context(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--fill" => {
                let raw = value("--fill")?;
                opts.fill =
                    FillMode::parse(&raw).with_context(|| format!("unknown fill mode {raw}"))?;
            }
            "--hwaccel" => opts.hwaccel = value("--hwaccel")?,
            "--ffmpeg" => opts.ffmpeg = value("--ffmpeg")?,
            "--ffprobe" => opts.ffprobe = value("--ffprobe")?,
            "--background" => opts.background = true,
            "--events" => opts.events = true,
            "-h" | "--help" => {
                println!(
                    "usage: wallflow-live VIDEO [--fill fill|fit|stretch] [--hwaccel auto|none|vaapi|cuda|...]\n\
                     \x20                    [--background] [--events] [--ffmpeg PATH] [--ffprobe PATH]"
                );
                std::process::exit(0);
            }
            other if other.starts_with('-') => bail!("unknown option {other}"),
            other => video = Some(PathBuf::from(other)),
        }
    }
    opts.video = video.context("no video given (see --help)")?;
    Ok(opts)
}

/// Commands from the Wallflow app, one JSON object per stdin line.
#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "camelCase")]
enum Control {
    Pause,
    Resume,
    Fill { mode: FillMode },
    Quit,
}

#[derive(Debug)]
enum UserEvent {
    Control(Control),
    StdinClosed,
}

/// Maps frame numbers to wall-clock times at the video's own rate.
struct Clock {
    start: Instant,
    shown: u64,
    fps: f64,
}

impl Clock {
    fn new(fps: f64) -> Self {
        Self {
            start: Instant::now(),
            shown: 0,
            fps,
        }
    }

    fn due(&self, frame: u64) -> Instant {
        self.start + Duration::from_secs_f64(frame as f64 / self.fps)
    }

    fn next_due(&self) -> Instant {
        self.due(self.shown)
    }

    fn restart(&mut self) {
        self.start = Instant::now();
        self.shown = 0;
    }
}

#[derive(Default)]
struct Stats {
    since: Option<Instant>,
    drawn: u32,
    dropped: u32,
    starved: u32,
}

struct Screen {
    window: Arc<Window>,
    target: Target,
}

struct App {
    opts: Opts,
    info: VideoInfo,
    instance: wgpu::Instance,
    gpu: Option<Gpu>,
    /// One per monitor in background mode, a single one otherwise.
    screens: Vec<Screen>,
    decoder: Decoder,
    clock: Clock,
    paused: bool,
    /// A frame was uploaded and no window has drawn it yet. No further frame is
    /// taken until one does, so when the compositor stops asking for frames
    /// (desktop covered everywhere) decoding stops too.
    awaiting_draw: bool,
    decoded_any: bool,
    decoder_restarts: u32,
    stats: Stats,
}

impl App {
    fn create_windows(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        if self.opts.background {
            let mut seen = Vec::new();
            for monitor in event_loop.available_monitors() {
                let position = monitor.position();
                if seen.contains(&position) {
                    continue; // mirrored outputs
                }
                seen.push(position);
                // The title carries the monitor's origin, which is how the
                // desktop shim knows where to put each window.
                let attrs = Window::default_attributes()
                    .with_title(format!("Wallflow Live @{},{}", position.x, position.y))
                    .with_decorations(false)
                    .with_position(position)
                    .with_inner_size(monitor.size());
                self.open_screen(event_loop, attrs)?;
            }
        }
        if self.screens.is_empty() {
            let attrs = Window::default_attributes()
                .with_title("Wallflow Live")
                .with_decorations(!self.opts.background)
                .with_inner_size(winit::dpi::LogicalSize::new(960.0, 540.0));
            self.open_screen(event_loop, attrs)?;
        }
        log::info!("drawing on {} window(s)", self.screens.len());
        events::emit(&Event::Ready {
            screens: self.screens.len(),
        });
        self.clock.restart();
        Ok(())
    }

    fn open_screen(
        &mut self,
        event_loop: &ActiveEventLoop,
        attrs: winit::window::WindowAttributes,
    ) -> Result<()> {
        #[cfg(target_os = "linux")]
        let attrs = {
            let attrs = winit::platform::wayland::WindowAttributesExtWayland::with_name(
                attrs, APP_ID, APP_ID,
            );
            winit::platform::x11::WindowAttributesExtX11::with_name(attrs, APP_ID, APP_ID)
        };

        let window = Arc::new(event_loop.create_window(attrs)?);
        if self.opts.background {
            // Clicks go through to the desktop (icons, context menu) underneath.
            if let Err(err) = window.set_cursor_hittest(false) {
                log::warn!("could not make the window click-through: {err}");
            }
        }
        let target = match &self.gpu {
            Some(gpu) => gpu.add_target(&self.instance, window.clone(), self.opts.fill)?,
            None => {
                let (gpu, target) =
                    Gpu::new(&self.instance, window.clone(), self.info, self.opts.fill)?;
                self.gpu = Some(gpu);
                target
            }
        };
        self.screens.push(Screen { window, target });
        Ok(())
    }

    /// Takes the frame that is due now, dropping any the display fell behind on.
    fn advance(&mut self) {
        let now = Instant::now();
        if now.saturating_duration_since(self.clock.next_due()) > MAX_LAG {
            self.clock.restart();
        }

        let mut latest: Option<Frame> = None;
        while self.clock.next_due() <= now {
            match self.decoder.try_next() {
                Ok(Some(frame)) => {
                    self.clock.shown += 1;
                    if let Some(old) = latest.replace(frame) {
                        self.stats.dropped += 1;
                        self.decoder.give_back(old);
                    }
                }
                Ok(None) => {
                    self.stats.starved += 1;
                    break;
                }
                Err(_) => {
                    self.restart_decoder();
                    break;
                }
            }
        }

        if latest.is_some() {
            self.decoded_any = true;
        }
        if let Some(frame) = latest {
            if let Some(gpu) = self.gpu.as_mut() {
                gpu.upload(&frame.data);
            }
            self.decoder.give_back(frame);
            self.awaiting_draw = true;
            for screen in &self.screens {
                screen.window.request_redraw();
            }
        }
    }

    /// FFmpeg exits on a broken file, a hardware decoder it cannot open, or a
    /// GPU reset. Try again, without hardware decoding if it never produced a
    /// frame, and give up only when restarting keeps failing.
    fn restart_decoder(&mut self) {
        self.decoder_restarts += 1;
        if self.decoder_restarts > MAX_DECODER_RESTARTS {
            fail("the decoder keeps stopping, giving up");
        }
        if !self.decoded_any && self.opts.hwaccel != "none" {
            log::warn!(
                "hardware decoding ({}) produced no frames, using software decoding",
                self.opts.hwaccel
            );
            self.opts.hwaccel = "none".into();
        } else {
            log::warn!("the decoder stopped, restarting it");
        }
        match Decoder::spawn(
            &self.opts.ffmpeg,
            &self.opts.video,
            &self.info,
            &self.opts.hwaccel,
        ) {
            Ok(decoder) => self.decoder = decoder,
            Err(err) => {
                fail(&format!("could not restart the decoder: {err:#}"));
            }
        }
        self.clock.restart();
    }

    fn log_stats(&mut self) {
        let now = Instant::now();
        let since = *self.stats.since.get_or_insert(now);
        if now - since < STATS_EVERY {
            return;
        }
        let secs = (now - since).as_secs_f64();
        log::info!(
            "drawn {:.1} fps per window, dropped {}, decoder late {}",
            self.stats.drawn as f64 / secs / self.screens.len().max(1) as f64,
            self.stats.dropped,
            self.stats.starved
        );
        self.stats = Stats {
            since: Some(now),
            ..Default::default()
        };
    }

    fn set_paused(&mut self, paused: bool, reason: &str) {
        if self.paused == paused {
            return;
        }
        self.paused = paused;
        if !paused {
            self.clock.restart();
        }
        events::emit(&Event::State { paused, reason });
    }

    fn control(&mut self, event_loop: &ActiveEventLoop, control: Control) {
        log::info!("command: {control:?}");
        match control {
            Control::Pause => self.set_paused(true, "app"),
            Control::Resume => self.set_paused(false, "app"),
            Control::Fill { mode } => {
                self.opts.fill = mode;
                if let Some(gpu) = &self.gpu {
                    for screen in &mut self.screens {
                        gpu.set_mode(&mut screen.target, mode);
                        screen.window.request_redraw();
                    }
                }
            }
            Control::Quit => event_loop.exit(),
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.screens.is_empty()
            && let Err(err) = self.create_windows(event_loop)
        {
            let message = format!("could not open the wallpaper window: {err:#}");
            log::error!("{message}");
            events::emit(&Event::Error { message: &message });
            event_loop.exit();
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Control(control) => self.control(event_loop, control),
            UserEvent::StdinClosed => {
                log::info!("the app went away, exiting");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(index) = self.screens.iter().position(|s| s.window.id() == id) else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gpu) = &self.gpu {
                    let screen = &mut self.screens[index];
                    gpu.resize(&mut screen.target, size.width, size.height);
                    screen.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(gpu) = &self.gpu
                    && gpu.draw(&self.screens[index].target)
                {
                    self.stats.drawn += 1;
                }
                self.awaiting_draw = false;
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.paused || self.gpu.is_none() {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        }
        if !self.awaiting_draw {
            self.advance();
        }
        self.log_stats();

        let wake = if self.awaiting_draw {
            // The redraw will wake us; this is only a safety net.
            Instant::now() + Duration::from_millis(100)
        } else if self.clock.next_due() <= Instant::now() {
            // The decoder is behind; check again shortly.
            Instant::now() + Duration::from_millis(2)
        } else {
            self.clock.next_due()
        };
        event_loop.set_control_flow(ControlFlow::WaitUntil(wake));
    }
}

fn listen_stdin(proxy: EventLoopProxy<UserEvent>) {
    std::thread::Builder::new()
        .name("wallflow-control".into())
        .spawn(move || {
            for line in std::io::stdin().lock().lines() {
                let Ok(line) = line else { break };
                if line.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<Control>(&line) {
                    Ok(control) => {
                        if proxy.send_event(UserEvent::Control(control)).is_err() {
                            return;
                        }
                    }
                    Err(err) => log::warn!("ignoring command {line:?}: {err}"),
                }
            }
            let _ = proxy.send_event(UserEvent::StdinClosed);
        })
        .expect("spawning the control thread");
}

fn parse_prepare(mut args: impl Iterator<Item = String>) -> Result<prepare::Args> {
    let mut input = None;
    let mut out = prepare::Args {
        input: PathBuf::new(),
        output: PathBuf::new(),
        poster: PathBuf::new(),
        screen: (1920, 1080),
        ffmpeg: "ffmpeg".into(),
        ffprobe: "ffprobe".into(),
    };
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().with_context(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--out" => out.output = value("--out")?.into(),
            "--poster" => out.poster = value("--poster")?.into(),
            "--width" => out.screen.0 = value("--width")?.parse().context("--width")?,
            "--height" => out.screen.1 = value("--height")?.parse().context("--height")?,
            "--ffmpeg" => out.ffmpeg = value("--ffmpeg")?,
            "--ffprobe" => out.ffprobe = value("--ffprobe")?,
            other if other.starts_with('-') => bail!("unknown option {other}"),
            other => input = Some(PathBuf::from(other)),
        }
    }
    out.input = input.context(
        "usage: wallflow-live prepare INPUT --out OUT.mp4 --poster OUT.jpg [--width W --height H]",
    )?;
    if out.output.as_os_str().is_empty() || out.poster.as_os_str().is_empty() {
        bail!("prepare needs --out and --poster");
    }
    Ok(out)
}

fn main() -> Result<()> {
    // Logs go to stderr; stdout is reserved for the JSON `prepare` reports.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    if std::env::args().nth(1).as_deref() == Some("prepare") {
        return prepare::run(&parse_prepare(std::env::args().skip(2))?);
    }
    let opts = parse_args()?;
    if opts.events {
        events::enable();
    }
    play(opts).inspect_err(|err| {
        events::emit(&Event::Error {
            message: &format!("{err:#}"),
        })
    })
}

/// Logs the reason, tells the app, and exits with status 2.
fn fail(message: &str) -> ! {
    log::error!("{message}");
    events::emit(&Event::Error { message });
    std::process::exit(2);
}

fn play(opts: Opts) -> Result<()> {
    let info = probe::probe(&opts.ffprobe, &opts.video)?;
    log::info!(
        "{}: {}x{} @ {:.2} fps",
        opts.video.display(),
        info.width,
        info.height,
        info.fps
    );
    let decoder = Decoder::spawn(&opts.ffmpeg, &opts.video, &info, &opts.hwaccel)?;

    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    listen_stdin(event_loop.create_proxy());

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(
        Box::new(event_loop.owned_display_handle()),
    ));

    let mut app = App {
        clock: Clock::new(info.fps),
        opts,
        info,
        instance,
        gpu: None,
        screens: Vec::new(),
        decoder,
        paused: false,
        awaiting_draw: false,
        decoded_any: false,
        decoder_restarts: 0,
        stats: Stats::default(),
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}
