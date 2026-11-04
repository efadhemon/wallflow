//! Live (video) wallpapers: a library of prepared videos, and playing one through
//! the `wallflow-live` engine.
//!
//! The engine is a separate binary bundled next to the app (a Tauri sidecar).
//! Videos are prepared once on import (`wallflow-live prepare`: screen-sized,
//! ≤30 fps, plus a poster frame) and stored in `<app data>/live/<id>/`. While a
//! live wallpaper plays, the image rotation is suspended and the poster is the
//! static wallpaper, so the lock screen and the moments before the engine is up
//! show the same picture.

#[cfg(target_os = "linux")]
pub mod gnome;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncBufReadExt, BufReader};

pub const IMPORT_EVENT: &str = "live://import";

const VIDEO_FILE: &str = "video.mp4";
const POSTER_FILE: &str = "poster.jpg";
const META_FILE: &str = "meta.json";

/// What the library remembers about one prepared video.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveMeta {
    pub id: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration: Option<f64>,
    pub added_at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveVideo {
    #[serde(flatten)]
    pub meta: LiveMeta,
    pub video_path: PathBuf,
    pub poster_path: PathBuf,
}

/// Progress of one import, sent as `IMPORT_EVENT`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportProgress {
    pub id: String,
    pub name: String,
    pub fraction: f64,
    /// "preparing", "done" or "failed".
    pub state: &'static str,
    pub error: Option<String>,
}

/// Can this desktop play live wallpapers, and if not, what is missing.
// Only GNOME has a shim yet; the Windows / macOS ones will construct these too.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Support {
    Ready,
    /// GNOME: the helper extension is not installed.
    NeedsSetup,
    /// GNOME: installed, but only loaded after the next login.
    NeedsLogin,
    Unsupported,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveStatus {
    pub support: Support,
    /// Why it is unsupported, when it is.
    pub reason: Option<String>,
    pub ffmpeg: bool,
    pub engine: bool,
    pub running: bool,
    /// Playing, but paused because the desktop is covered.
    pub paused: bool,
    pub active: Option<String>,
}

pub struct Live {
    dir: PathBuf,
    /// Ids being imported, so the same file is not prepared twice at once.
    importing: Mutex<Vec<String>>,
}

impl Live {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            importing: Mutex::new(Vec::new()),
        }
    }

    pub fn list(&self) -> Vec<LiveVideo> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut videos: Vec<LiveVideo> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| self.load(&entry.file_name().to_string_lossy()))
            .collect();
        videos.sort_by_key(|video| std::cmp::Reverse(video.meta.added_at));
        videos
    }

    pub fn get(&self, id: &str) -> Option<LiveVideo> {
        self.load(id)
    }

    fn load(&self, id: &str) -> Option<LiveVideo> {
        // Hidden entries are imports still in progress.
        if id.starts_with('.') || id.contains(['/', '\\']) {
            return None;
        }
        let dir = self.dir.join(id);
        let raw = std::fs::read_to_string(dir.join(META_FILE)).ok()?;
        let meta: LiveMeta = serde_json::from_str(&raw).ok()?;
        let video_path = dir.join(VIDEO_FILE);
        let poster_path = dir.join(POSTER_FILE);
        (video_path.is_file() && poster_path.is_file()).then_some(LiveVideo {
            meta,
            video_path,
            poster_path,
        })
    }

    pub fn remove(&self, id: &str) -> Result<()> {
        let video = self
            .get(id)
            .context("that live wallpaper is not in the library")?;
        let dir = video.video_path.parent().context("no folder")?;
        std::fs::remove_dir_all(dir).with_context(|| format!("removing {}", dir.display()))
    }

    /// Prepares `source` with the engine and adds it to the library, reporting
    /// progress as it goes. Importing the same file again returns the existing entry.
    pub async fn import(
        &self,
        on_progress: &(dyn Fn(ImportProgress) + Send + Sync),
        engine: &Path,
        source: &Path,
        screen: (u32, u32),
    ) -> Result<LiveVideo> {
        let id = tokio::task::spawn_blocking({
            let source = source.to_path_buf();
            move || content_id(&source)
        })
        .await??;
        if let Some(existing) = self.get(&id) {
            return Ok(existing);
        }
        {
            let mut importing = self.importing.lock().expect("live poisoned");
            if importing.contains(&id) {
                bail!("That video is already being added.");
            }
            importing.push(id.clone());
        }
        let result = self.prepare(on_progress, engine, source, screen, &id).await;
        self.importing
            .lock()
            .expect("live poisoned")
            .retain(|other| other != &id);
        result
    }

    async fn prepare(
        &self,
        on_progress: &(dyn Fn(ImportProgress) + Send + Sync),
        engine: &Path,
        source: &Path,
        screen: (u32, u32),
        id: &str,
    ) -> Result<LiveVideo> {
        let name = source
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Video".into());
        let report = |fraction: f64, state: &'static str, error: Option<String>| {
            on_progress(ImportProgress {
                id: id.to_string(),
                name: name.clone(),
                fraction,
                state,
                error,
            });
        };
        report(0.0, "preparing", None);

        let staging = self.dir.join(format!(".{id}.importing"));
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging)?;

        let outcome = async {
            let mut child = tokio::process::Command::new(engine)
                .arg("prepare")
                .arg(source)
                .arg("--out")
                .arg(staging.join(VIDEO_FILE))
                .arg("--poster")
                .arg(staging.join(POSTER_FILE))
                .args([
                    "--width",
                    &screen.0.to_string(),
                    "--height",
                    &screen.1.to_string(),
                ])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .with_context(|| format!("starting {}", engine.display()))?;

            let mut lines = BufReader::new(child.stdout.take().context("no stdout")?).lines();
            let mut started: Option<serde_json::Value> = None;
            while let Some(line) = lines.next_line().await? {
                let Ok(event) = serde_json::from_str::<serde_json::Value>(&line) else {
                    continue;
                };
                match event["event"].as_str() {
                    Some("started") => started = Some(event),
                    Some("progress") => {
                        report(event["fraction"].as_f64().unwrap_or(0.0), "preparing", None)
                    }
                    _ => {}
                }
            }
            let output = child.wait_with_output().await?;
            if !output.status.success() {
                let detail = String::from_utf8_lossy(&output.stderr);
                let detail = detail.lines().rev().find(|line| !line.trim().is_empty());
                bail!(
                    "Could not prepare {}: {}",
                    source.display(),
                    detail.unwrap_or("the video could not be read")
                );
            }
            let started = started.context("the engine reported no video details")?;
            Ok(LiveMeta {
                id: id.to_string(),
                name: name.clone(),
                width: started["width"].as_u64().unwrap_or(0) as u32,
                height: started["height"].as_u64().unwrap_or(0) as u32,
                fps: started["fps"].as_f64().unwrap_or(0.0),
                duration: started["duration"].as_f64(),
                added_at: crate::state::now_ms(),
            })
        }
        .await;

        match outcome {
            Ok(meta) => {
                std::fs::write(staging.join(META_FILE), serde_json::to_vec_pretty(&meta)?)?;
                let dir = self.dir.join(id);
                std::fs::rename(&staging, &dir)
                    .with_context(|| format!("saving {}", dir.display()))?;
                report(1.0, "done", None);
                self.get(id).context("the prepared video went missing")
            }
            Err(err) => {
                let _ = std::fs::remove_dir_all(&staging);
                report(0.0, "failed", Some(format!("{err:#}")));
                Err(err)
            }
        }
    }
}

/// Identifies a video by its size and a hash of its start and end, which is
/// enough to recognise the same file without reading gigabytes.
fn content_id(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::{Read, Seek, SeekFrom};
    const SAMPLE: u64 = 4 << 20;

    let mut file =
        std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let len = file.metadata()?.len();
    let mut hasher = Sha256::new();
    hasher.update(len.to_le_bytes());
    let mut buf = vec![0; SAMPLE.min(len) as usize];
    file.read_exact(&mut buf)?;
    hasher.update(&buf);
    if len > SAMPLE * 2 {
        file.seek(SeekFrom::End(-(SAMPLE as i64)))?;
        file.read_exact(&mut buf)?;
        hasher.update(&buf);
    }
    let digest = hasher.finalize();
    Ok(digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// The bundled engine: Tauri puts sidecars next to the app's own executable.
pub fn engine_path() -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let path = exe.with_file_name(format!("wallflow-live{}", std::env::consts::EXE_SUFFIX));
    if !path.is_file() {
        bail!(
            "The live wallpaper engine is missing from this installation ({}).",
            path.display()
        );
    }
    Ok(path)
}

/// FFmpeg decodes and prepares the videos; it is a package dependency on Linux.
/// A success is remembered (the Live page asks every few seconds); a failure is
/// checked again, so installing FFmpeg takes effect without a restart.
pub fn ffmpeg_available() -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    static FOUND: AtomicBool = AtomicBool::new(false);
    if FOUND.load(Ordering::Relaxed) {
        return true;
    }
    let found = ["ffmpeg", "ffprobe"].iter().all(|tool| {
        std::process::Command::new(tool)
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    });
    FOUND.store(found, Ordering::Relaxed);
    found
}

/// The largest monitor, which the import prepares videos for.
pub fn screen_size(app: &AppHandle) -> (u32, u32) {
    let sizes: Vec<(u32, u32)> = app
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|monitor| (monitor.size().width, monitor.size().height))
        .collect();
    let width = sizes.iter().map(|size| size.0).max().unwrap_or(1920);
    let height = sizes.iter().map(|size| size.1).max().unwrap_or(1080);
    (width.max(1), height.max(1))
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn engine_fill(mode: crate::config::FillMode) -> &'static str {
    use crate::config::FillMode;
    match mode {
        FillMode::Fill => "fill",
        FillMode::Stretch => "stretch",
        FillMode::Fit | FillMode::Center | FillMode::Tile => "fit",
    }
}

pub fn status(active: Option<String>) -> LiveStatus {
    let (support, reason, shim) = platform_support();
    LiveStatus {
        support,
        reason,
        ffmpeg: ffmpeg_available(),
        engine: engine_path().is_ok(),
        running: shim.running,
        paused: shim.paused,
        active,
    }
}

#[derive(Default)]
struct Shim {
    running: bool,
    paused: bool,
}

#[cfg(target_os = "linux")]
fn platform_support() -> (Support, Option<String>, Shim) {
    if let Err(reason) = gnome::session_supported() {
        return (Support::Unsupported, Some(reason), Shim::default());
    }
    match gnome::state() {
        Some(state) => (
            Support::Ready,
            None,
            Shim {
                running: state.running,
                paused: state.paused,
            },
        ),
        None if gnome::installed() => (Support::NeedsLogin, None, Shim::default()),
        None => (Support::NeedsSetup, None, Shim::default()),
    }
}

#[cfg(not(target_os = "linux"))]
fn platform_support() -> (Support, Option<String>, Shim) {
    (
        Support::Unsupported,
        Some("Live wallpapers are coming to this system soon.".into()),
        Shim::default(),
    )
}

pub fn start(video: &LiveVideo, fill: crate::config::FillMode) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        gnome::ensure_ready()?;
        let engine = engine_path()?;
        gnome::start(&[
            engine.to_string_lossy().into_owned(),
            video.video_path.to_string_lossy().into_owned(),
            "--background".into(),
            "--fill".into(),
            engine_fill(fill).into(),
        ])
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (video, fill);
        bail!("Live wallpapers are not available on this system yet.")
    }
}

pub fn stop() -> Result<()> {
    #[cfg(target_os = "linux")]
    if gnome::state().is_some_and(|state| state.running) {
        return gnome::stop();
    }
    Ok(())
}

pub fn set_fill(fill: crate::config::FillMode) {
    #[cfg(target_os = "linux")]
    {
        let line = format!(r#"{{"cmd":"fill","mode":"{}"}}"#, engine_fill(fill));
        if let Err(err) = gnome::send(&line) {
            log::warn!("could not change the live wallpaper's fill mode: {err:#}");
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = fill;
}

pub fn install_helper() -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        gnome::install()
    }
    #[cfg(not(target_os = "linux"))]
    bail!("No helper is needed on this system.")
}

/// Keeps the chosen live wallpaper playing. After login the shell (and with it
/// the shim) may come up after Wallflow; gnome-shell can restart; the engine
/// can exit. Whenever a live wallpaper is selected, the shim is loaded and
/// nothing is playing, it is started again.
pub async fn supervise(app: AppHandle) {
    use crate::state::AppState;
    use std::sync::Arc;
    use std::time::Duration;

    let state = app.state::<Arc<AppState>>().inner().clone();
    // Often at first, so a login restores it quickly; then a cheap heartbeat.
    let mut checks = 0u32;
    loop {
        let delay = if checks < 30 { 2 } else { 15 };
        checks = checks.saturating_add(1);

        if let Some(id) = state.config().live_video {
            let (support, _, shim) = platform_support();
            if support == Support::Ready && !shim.running {
                match state.live.get(&id) {
                    Some(video) => {
                        let fill = state.config().fill_mode;
                        let started =
                            tokio::task::spawn_blocking(move || start(&video, fill)).await;
                        match started {
                            Ok(Ok(())) => log::info!("started the live wallpaper {id}"),
                            Ok(Err(err)) => {
                                log::warn!("could not start the live wallpaper: {err:#}")
                            }
                            Err(err) => log::warn!("could not start the live wallpaper: {err}"),
                        }
                        crate::scheduler::publish(&app, &state);
                    }
                    None => log::warn!("the live wallpaper {id} is no longer in the library"),
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(delay)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_id_is_stable_and_size_aware() {
        let dir = std::env::temp_dir().join(format!("wallflow-live-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.bin");
        std::fs::write(&a, b"same bytes").unwrap();
        let b = dir.join("b.bin");
        std::fs::write(&b, b"same bytes").unwrap();
        let c = dir.join("c.bin");
        std::fs::write(&c, b"same bytes!").unwrap();
        assert_eq!(content_id(&a).unwrap(), content_id(&b).unwrap());
        assert_ne!(content_id(&a).unwrap(), content_id(&c).unwrap());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn library_skips_unfinished_imports() {
        let dir = std::env::temp_dir().join(format!("wallflow-live-lib-{}", std::process::id()));
        let done = dir.join("abc");
        std::fs::create_dir_all(&done).unwrap();
        std::fs::create_dir_all(dir.join(".def.importing")).unwrap();
        let meta = LiveMeta {
            id: "abc".into(),
            name: "Clip".into(),
            width: 1920,
            height: 1080,
            fps: 30.0,
            duration: Some(4.0),
            added_at: 1,
        };
        std::fs::write(done.join(META_FILE), serde_json::to_vec(&meta).unwrap()).unwrap();
        std::fs::write(done.join(VIDEO_FILE), b"v").unwrap();
        std::fs::write(done.join(POSTER_FILE), b"p").unwrap();

        let live = Live::new(dir.clone());
        let list = live.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].meta.name, "Clip");
        assert!(live.get("../abc").is_none());
        live.remove("abc").unwrap();
        assert!(live.list().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

/// End-to-end checks against the real engine. Ignored by default: they need the
/// built sidecar, FFmpeg and a sample video (`WALLFLOW_TEST_VIDEO`), and the
/// shim test needs a GNOME session with the shim loaded.
///
///   WALLFLOW_TEST_VIDEO=~/Videos/clip.mp4 cargo test -- --ignored --test-threads 1
#[cfg(test)]
mod end_to_end {
    use super::*;
    use std::sync::{Arc, Mutex as StdMutex};

    fn sidecar() -> PathBuf {
        let triple = env!("WALLFLOW_TARGET_TRIPLE");
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("binaries/wallflow-live-{triple}"))
    }

    fn sample() -> PathBuf {
        PathBuf::from(std::env::var("WALLFLOW_TEST_VIDEO").expect("set WALLFLOW_TEST_VIDEO"))
    }

    #[tokio::test]
    #[ignore]
    async fn imports_a_real_video_once() {
        let dir = std::env::temp_dir().join(format!("wallflow-e2e-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let live = Live::new(dir.clone());
        let events: Arc<StdMutex<Vec<ImportProgress>>> = Arc::default();
        let sink = events.clone();
        let on_progress = move |progress: ImportProgress| sink.lock().unwrap().push(progress);

        let video = live
            .import(&on_progress, &sidecar(), &sample(), (1280, 720))
            .await
            .expect("import failed");
        assert!(video.video_path.is_file() && video.poster_path.is_file());
        assert!(video.meta.width <= 1280 + 2 && video.meta.height <= 720 + 2);
        assert!(video.meta.fps > 0.0 && video.meta.fps <= 30.5);
        assert!(video.meta.duration.is_some_and(|secs| secs > 0.0));

        let states: Vec<&str> = events.lock().unwrap().iter().map(|e| e.state).collect();
        assert_eq!(states.first(), Some(&"preparing"));
        assert_eq!(states.last(), Some(&"done"));

        // Same file again: no second prepare, same entry.
        let before = events.lock().unwrap().len();
        let again = live
            .import(&on_progress, &sidecar(), &sample(), (1280, 720))
            .await
            .unwrap();
        assert_eq!(again.meta.id, video.meta.id);
        assert_eq!(events.lock().unwrap().len(), before);
        assert_eq!(live.list().len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore]
    fn plays_and_stops_through_the_shim() {
        gnome::ensure_ready().expect("the GNOME shim is not loaded in this session");
        let video = LiveVideo {
            meta: LiveMeta {
                id: "e2e".into(),
                name: "e2e".into(),
                width: 0,
                height: 0,
                fps: 0.0,
                duration: None,
                added_at: 0,
            },
            video_path: sample(),
            poster_path: PathBuf::new(),
        };
        let engine = sidecar();
        gnome::start(&[
            engine.to_string_lossy().into_owned(),
            video.video_path.to_string_lossy().into_owned(),
            "--background".into(),
            "--fill".into(),
            "fit".into(),
        ])
        .expect("start failed");
        std::thread::sleep(std::time::Duration::from_secs(3));
        assert!(gnome::state().is_some_and(|state| state.running));
        set_fill(crate::config::FillMode::Fill);
        gnome::stop().unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1));
        assert!(gnome::state().is_some_and(|state| !state.running));
    }
}
