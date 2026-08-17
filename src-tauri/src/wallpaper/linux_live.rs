use super::gnome;
use super::linux::is_x11;
use super::live::file_uri;
use super::live::LiveOpts;
use super::wayland_live;
use anyhow::{anyhow, bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{self, ConnectionExt as _, Window};
use x11rb::wrapper::ConnectionExt as _;

pub struct Handle {
    stop: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    opts: Arc<Mutex<LiveOpts>>,
    path: PathBuf,
    child: Arc<Mutex<Option<Child>>>,
    thread: Option<JoinHandle<()>>,
    gnome: bool,
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if self.gnome {
            gnome::stop();
        }
        kill_child(&self.child);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Handle {
    pub fn pause(&self, paused: bool) {
        self.pause.store(paused, Ordering::SeqCst);
        if self.gnome {
            gnome::pause(paused);
        }
        if paused {
            kill_child(&self.child);
        }
    }

    pub fn update_opts(&self, opts: LiveOpts) {
        if let Ok(mut slot) = self.opts.lock() {
            *slot = opts;
        }
        if self.gnome {
            if let Err(err) = gnome::set_video(&self.path, opts) {
                log::warn!("could not update the live wallpaper: {err:#}");
            }
        } else {
            kill_child(&self.child);
        }
    }
}

fn kill_child(child: &Mutex<Option<Child>>) {
    if let Ok(mut slot) = child.lock() {
        if let Some(mut proc) = slot.take() {
            let _ = proc.kill();
            let _ = proc.wait();
        }
    }
}

pub fn start(path: &Path, opts: LiveOpts) -> Result<Handle> {
    if gnome::is_gnome_shell() && (gnome::is_wayland() || gnome::ping()) {
        gnome::set_video(path, opts)?;
        return Ok(Handle {
            stop: Arc::new(AtomicBool::new(false)),
            pause: Arc::new(AtomicBool::new(false)),
            opts: Arc::new(Mutex::new(opts)),
            path: path.to_path_buf(),
            child: Arc::new(Mutex::new(None)),
            thread: None,
            gnome: true,
        });
    }

    let use_wayland = !is_x11();
    if use_wayland && !wayland_live::available() {
        bail!(
            "Live wallpaper on this Wayland session needs either GNOME (with the Wallflow extension) or a compositor that supports wlr-layer-shell."
        );
    }

    if !command_exists("gst-play-1.0") && !command_exists("gst-launch-1.0") {
        bail!(
            "GStreamer is not installed. Install gstreamer1.0-plugins-base-apps and gstreamer1.0-libav."
        );
    }

    let path_buf = path.to_path_buf();
    let stop = Arc::new(AtomicBool::new(false));
    let pause = Arc::new(AtomicBool::new(false));
    let opts = Arc::new(Mutex::new(opts));
    let child = Arc::new(Mutex::new(None));
    let stop_t = stop.clone();
    let pause_t = pause.clone();
    let opts_t = opts.clone();
    let child_t = child.clone();
    let path_t = path_buf.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();

    let thread = thread::Builder::new()
        .name("wallflow-live".into())
        .spawn(move || {
            let result = if use_wayland {
                wayland_live::run(&path_t, &opts_t, &stop_t, &pause_t, &child_t, ready_tx)
            } else {
                run_x11_loop(&path_t, &opts_t, &stop_t, &pause_t, &child_t, ready_tx)
            };
            if let Err(err) = result {
                log::error!("live wallpaper stopped: {err:#}");
            }
        })
        .context("starting the live wallpaper thread")?;

    match ready_rx.recv_timeout(Duration::from_secs(12)) {
        Ok(Ok(())) => {}
        Ok(Err(err)) => {
            stop.store(true, Ordering::SeqCst);
            kill_child(&child);
            let _ = thread.join();
            return Err(err);
        }
        Err(_) => {
            stop.store(true, Ordering::SeqCst);
            kill_child(&child);
            let _ = thread.join();
            bail!("timed out starting the live wallpaper");
        }
    }

    Ok(Handle {
        stop,
        pause,
        opts,
        path: path_buf,
        child,
        thread: Some(thread),
        gnome: false,
    })
}

pub fn extract_poster(cache_dir: &Path, source: &Path) -> Result<PathBuf> {
    let target = super::live::poster_path(cache_dir, source)?;
    if target.is_file() {
        return Ok(target);
    }

    if grab_poster_with_gst(source, &target).is_err() {
        super::live::write_placeholder_poster(&target)?;
    }
    Ok(target)
}

fn grab_poster_with_gst(source: &Path, target: &Path) -> Result<()> {
    let uri = file_uri(source);
    let mut command = Command::new("gst-launch-1.0");
    command
        .args([
            "-q",
            "uridecodebin",
            &format!("uri={uri}"),
            "n-video=1",
            "n-audio=0",
            "!",
            "queue",
            "max-size-buffers=1",
            "leaky=downstream",
            "!",
            "videoconvert",
            "!",
            "videoscale",
            "!",
            "jpegenc",
            "snapshot=true",
            "quality=80",
            "!",
            "filesink",
            &format!("location={}", target.display()),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let status = command.status();

    match status {
        Ok(code) if code.success() && target.is_file() => Ok(()),
        _ => bail!("gst-launch could not extract a poster frame"),
    }
}

fn run_x11_loop(
    path: &Path,
    opts: &Mutex<LiveOpts>,
    stop: &AtomicBool,
    pause: &AtomicBool,
    child_slot: &Mutex<Option<Child>>,
    ready: std::sync::mpsc::Sender<Result<()>>,
) -> Result<()> {
    let mut announced = false;
    while !stop.load(Ordering::SeqCst) {
        if pause.load(Ordering::SeqCst) {
            kill_child(child_slot);
            thread::sleep(Duration::from_millis(120));
            continue;
        }

        let muted = opts.lock().map(|guard| guard.muted).unwrap_or(true);
        let child = match spawn_player(path, muted) {
            Ok(child) => child,
            Err(err) => {
                if !announced {
                    let _ = ready.send(Err(anyhow!("{err:#}")));
                    return Err(err);
                }
                thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        let pid = child.id();
        if let Err(err) = restyle_player_window(pid) {
            log::warn!("could not place the live wallpaper window: {err:#}");
        }
        if !announced {
            let _ = ready.send(Ok(()));
            announced = true;
        }
        *child_slot.lock().expect("child lock") = Some(child);

        loop {
            if stop.load(Ordering::SeqCst) || pause.load(Ordering::SeqCst) {
                kill_child(child_slot);
                break;
            }
            let finished = {
                let mut slot = child_slot.lock().expect("child lock");
                match slot.as_mut() {
                    Some(proc) => matches!(proc.try_wait(), Ok(Some(_))),
                    None => true,
                }
            };
            if finished {
                let _ = child_slot.lock().expect("child lock").take();
                break;
            }
            thread::sleep(Duration::from_millis(200));
        }
    }
    Ok(())
}

fn spawn_player(path: &Path, muted: bool) -> Result<Child> {
    let uri = file_uri(path);
    if command_exists("gst-play-1.0") {
        let mut command = Command::new("gst-play-1.0");
        command.args(["--loop", "--quiet"]);
        if muted {
            command.args(["--volume=0.0", "--audiosink=fakesink"]);
        }
        command.arg(&uri);
        command.stdin(Stdio::null());
        command.stdout(Stdio::null());
        command.stderr(Stdio::null());
        return command.spawn().context("starting gst-play-1.0");
    }

    let mut command = Command::new("gst-launch-1.0");
    command.args([
        "-q",
        "playbin",
        &format!("uri={uri}"),
        "video-sink=ximagesink",
    ]);
    if muted {
        command.arg("mute=true");
    }
    command.stdin(Stdio::null());
    command.stdout(Stdio::null());
    command.stderr(Stdio::null());
    command.spawn().context("starting gst-launch-1.0")
}

fn restyle_player_window(pid: u32) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(6);
    let (conn, screen_num) = x11rb::connect(None).context("connecting to X11")?;
    let screen = &conn.setup().roots[screen_num];
    let net_pid = conn.intern_atom(false, b"_NET_WM_PID")?.reply()?.atom;
    let type_atom = conn
        .intern_atom(false, b"_NET_WM_WINDOW_TYPE")?
        .reply()?
        .atom;
    let desktop_atom = conn
        .intern_atom(false, b"_NET_WM_WINDOW_TYPE_DESKTOP")?
        .reply()?
        .atom;
    let state_atom = conn.intern_atom(false, b"_NET_WM_STATE")?.reply()?.atom;
    let below_atom = conn
        .intern_atom(false, b"_NET_WM_STATE_BELOW")?
        .reply()?
        .atom;
    let skip_taskbar = conn
        .intern_atom(false, b"_NET_WM_STATE_SKIP_TASKBAR")?
        .reply()?
        .atom;
    let skip_pager = conn
        .intern_atom(false, b"_NET_WM_STATE_SKIP_PAGER")?
        .reply()?
        .atom;

    let win = loop {
        if Instant::now() > deadline {
            bail!("could not find the GStreamer window");
        }
        if let Some(win) = find_window_by_pid(&conn, screen.root, net_pid, pid)? {
            break win;
        }
        thread::sleep(Duration::from_millis(80));
    };

    conn.change_property32(
        xproto::PropMode::REPLACE,
        win,
        type_atom,
        xproto::AtomEnum::ATOM,
        &[desktop_atom],
    )?;
    conn.change_property32(
        xproto::PropMode::REPLACE,
        win,
        state_atom,
        xproto::AtomEnum::ATOM,
        &[below_atom, skip_taskbar, skip_pager],
    )?;
    conn.configure_window(
        win,
        &xproto::ConfigureWindowAux::new()
            .x(0)
            .y(0)
            .width(u32::from(screen.width_in_pixels))
            .height(u32::from(screen.height_in_pixels))
            .stack_mode(xproto::StackMode::BELOW),
    )?;
    conn.flush()?;
    Ok(())
}

fn find_window_by_pid<C: Connection>(
    conn: &C,
    root: Window,
    pid_atom: xproto::Atom,
    pid: u32,
) -> Result<Option<Window>> {
    let mut stack = vec![root];
    while let Some(win) = stack.pop() {
        if let Ok(reply) = conn
            .get_property(false, win, pid_atom, xproto::AtomEnum::CARDINAL, 0, 1)?
            .reply()
        {
            if let Some(value) = reply.value32().and_then(|mut values| values.next()) {
                if value == pid {
                    return Ok(Some(win));
                }
            }
        }
        if let Ok(tree) = conn.query_tree(win)?.reply() {
            stack.extend(tree.children);
        }
    }
    Ok(None)
}

fn command_exists(name: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {name}"))
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}
