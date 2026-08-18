use crate::config::{self, Config, Folder, WallpaperMode};
use crate::gallery::{self, GalleryPage, SearchParams};
use crate::scanner;
use crate::state::{AppState, Command, Status};
use crate::wallpaper::live::LiveSupport;
use crate::{thumbs, watcher};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_autostart::ManagerExt;

type Reply<T> = Result<T, String>;

fn fail(err: impl std::fmt::Display) -> String {
    format!("{err:#}")
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageEntry {
    pub path: PathBuf,
    pub name: String,
    pub excluded: bool,
    pub is_current: bool,
    pub managed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderEntry {
    pub path: PathBuf,
    pub recursive: bool,
    pub exists: bool,
    pub image_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    pub os: String,
    pub backend: String,
    pub monitor_count: usize,
    pub supports_per_monitor: bool,
    pub supports_live: bool,
    pub live_support: &'static str,
    pub gnome_extension_status: &'static str,
    pub app_version: String,
    pub config_path: String,
    pub log_path: String,
    pub cache_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddRemoteResult {
    pub added: bool,
    pub path: PathBuf,
}

#[tauri::command]
pub fn get_status(state: State<'_, Arc<AppState>>) -> Status {
    state.status()
}

#[tauri::command]
pub fn get_config(state: State<'_, Arc<AppState>>) -> Config {
    state.config()
}

/// Persists the whole settings object, then tells the scheduler to rebuild.
#[tauri::command]
pub fn set_config(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    mut config: Config,
) -> Reply<Config> {
    config.sanitize();
    config.save(&state.config_dir).map_err(fail)?;

    let folders_changed = {
        let mut inner = state.inner.lock().expect("state poisoned");
        let changed = inner.config.folders != config.folders;
        inner.config = config.clone();
        changed
    };

    apply_autostart(&app, config.autostart);
    if folders_changed {
        watcher::rewatch(&state);
    }
    state.send(Command::Reload);

    Ok(config)
}

#[tauri::command]
pub fn add_folder(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    path: PathBuf,
    recursive: bool,
) -> Reply<Config> {
    if !path.is_dir() {
        return Err(format!("{} is not a folder", path.display()));
    }

    let config = {
        let mut inner = state.inner.lock().expect("state poisoned");
        if inner
            .config
            .folders
            .iter()
            .any(|folder| folder.path == path)
        {
            return Err("That folder is already in the list.".to_string());
        }
        inner.config.folders.push(Folder { path, recursive });
        inner.config.clone()
    };

    persist_and_reload(&app, &state, config)
}

#[tauri::command]
pub fn remove_folder(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    path: PathBuf,
) -> Reply<Config> {
    let config = {
        let mut inner = state.inner.lock().expect("state poisoned");
        inner.config.folders.retain(|folder| folder.path != path);
        inner.config.clone()
    };

    persist_and_reload(&app, &state, config)
}

#[tauri::command]
pub fn set_folder_recursive(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    path: PathBuf,
    recursive: bool,
) -> Reply<Config> {
    let config = {
        let mut inner = state.inner.lock().expect("state poisoned");
        if let Some(folder) = inner.config.folders.iter_mut().find(|f| f.path == path) {
            folder.recursive = recursive;
        }
        inner.config.clone()
    };

    persist_and_reload(&app, &state, config)
}

#[tauri::command]
pub async fn list_folders(state: State<'_, Arc<AppState>>) -> Reply<Vec<FolderEntry>> {
    let folders = state.config().folders;

    tokio::task::spawn_blocking(move || {
        folders
            .into_iter()
            .map(|folder| FolderEntry {
                exists: folder.path.is_dir(),
                image_count: scanner::count(&folder),
                path: folder.path,
                recursive: folder.recursive,
            })
            .collect()
    })
    .await
    .map_err(fail)
}

#[tauri::command]
pub fn list_images(state: State<'_, Arc<AppState>>) -> Vec<ImageEntry> {
    let images_dir = state.images_dir.as_path();
    let inner = state.inner.lock().expect("state poisoned");
    inner
        .images
        .iter()
        .map(|path| ImageEntry {
            name: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            excluded: inner.config.is_excluded(path),
            is_current: inner.current.as_deref() == Some(path.as_path()),
            managed: path.parent() == Some(images_dir),
            path: path.clone(),
        })
        .collect()
}

/// Copies picked files into the managed images directory as `{sha256}.{ext}`.
#[tauri::command]
pub async fn add_images(state: State<'_, Arc<AppState>>, paths: Vec<PathBuf>) -> Reply<usize> {
    let images_dir = state.images_dir.clone();
    let added = tokio::task::spawn_blocking(move || copy_images(&images_dir, &paths))
        .await
        .map_err(fail)??;
    state.send(Command::Reload);
    Ok(added)
}

/// Deletes a copy from the managed directory. Folder-sourced images are refused.
#[tauri::command]
pub fn remove_image(state: State<'_, Arc<AppState>>, path: PathBuf) -> Reply<()> {
    let images_dir = state.images_dir.canonicalize().map_err(fail)?;
    let canonical = path.canonicalize().map_err(fail)?;
    if !canonical.starts_with(&images_dir) {
        return Err("That image is not one Wallflow added.".into());
    }

    std::fs::remove_file(&canonical).map_err(fail)?;

    let config = {
        let mut inner = state.inner.lock().expect("state poisoned");
        inner
            .config
            .excluded
            .retain(|entry| entry != &path && entry != &canonical);
        inner.config.clone()
    };
    config.save(&state.config_dir).map_err(fail)?;
    state.send(Command::Reload);
    Ok(())
}

/// Decoding happens off the async runtime; the UI asks for these one tile at a time.
#[tauri::command]
pub async fn get_thumbnail(state: State<'_, Arc<AppState>>, path: PathBuf) -> Reply<String> {
    let cache_dir = state.cache_dir.clone();
    let is_video = scanner::is_video(&path);
    let app_state = Arc::clone(&state);

    tokio::task::spawn_blocking(move || {
        if is_video {
            app_state.backend.extract_poster(&cache_dir, &path)
        } else {
            thumbs::thumbnail(&cache_dir, &path)
        }
    })
    .await
    .map_err(fail)?
    .map(|path| path.to_string_lossy().into_owned())
    .map_err(fail)
}

#[tauri::command]
pub fn apply_image(state: State<'_, Arc<AppState>>, path: PathBuf) -> Reply<()> {
    if !path.is_file() {
        return Err(format!("{} no longer exists", path.display()));
    }

    {
        let mut inner = state.inner.lock().expect("state poisoned");
        if let Some(index) = inner.playlist.iter().position(|entry| entry == &path) {
            inner.index = index;
        }
        inner.current = Some(path);
    }

    state.send(Command::ApplyCurrent);
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_current: bool,
}

#[tauri::command]
pub fn list_videos(state: State<'_, Arc<AppState>>) -> Vec<VideoEntry> {
    let inner = state.inner.lock().expect("state poisoned");
    let current = inner.config.live_path.as_deref();
    inner
        .config
        .live_videos
        .iter()
        .filter(|path| path.is_file())
        .map(|path| VideoEntry {
            name: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            is_current: current == Some(path.as_path())
                && inner.config.wallpaper_mode == WallpaperMode::Live,
            path: path.clone(),
        })
        .collect()
}

#[tauri::command]
pub async fn add_videos(state: State<'_, Arc<AppState>>, paths: Vec<PathBuf>) -> Reply<usize> {
    let videos_dir = state.videos_dir.clone();
    let copied = tokio::task::spawn_blocking(move || copy_videos(&videos_dir, &paths))
        .await
        .map_err(fail)??;

    let mut inner = state.inner.lock().expect("state poisoned");
    let mut added = 0;
    for path in copied {
        if !inner.config.live_videos.contains(&path) {
            inner.config.live_videos.push(path);
            added += 1;
        }
    }
    let config = inner.config.clone();
    drop(inner);
    config.save(&state.config_dir).map_err(fail)?;
    Ok(added)
}

#[tauri::command]
pub fn remove_video(state: State<'_, Arc<AppState>>, path: PathBuf) -> Reply<()> {
    let videos_dir = state.videos_dir.canonicalize().map_err(fail)?;
    let canonical = path.canonicalize().map_err(fail)?;
    if !canonical.starts_with(&videos_dir) {
        return Err("That video is not one Wallflow added.".into());
    }

    let mut inner = state.inner.lock().expect("state poisoned");
    inner
        .config
        .live_videos
        .retain(|entry| entry != &path && entry != &canonical);
    let was_current = inner.config.live_path.as_deref() == Some(canonical.as_path())
        || inner.config.live_path.as_deref() == Some(path.as_path());
    if was_current {
        inner.config.live_path = None;
        if inner.config.wallpaper_mode == WallpaperMode::Live {
            inner.config.wallpaper_mode = WallpaperMode::Rotation;
        }
    }
    let config = inner.config.clone();
    drop(inner);

    if was_current {
        state.backend.stop_live();
        std::thread::sleep(std::time::Duration::from_millis(80));
    }

    std::fs::remove_file(&canonical).map_err(fail)?;
    config.save(&state.config_dir).map_err(fail)?;
    if was_current {
        state.send(Command::ApplyCurrent);
    }
    Ok(())
}

#[tauri::command]
pub fn apply_live(state: State<'_, Arc<AppState>>, path: PathBuf) -> Reply<()> {
    if !path.is_file() {
        return Err(format!("{} no longer exists", path.display()));
    }
    if !scanner::is_video(&path) {
        return Err("That file is not a supported video (MP4, MOV or M4V).".into());
    }

    {
        let mut inner = state.inner.lock().expect("state poisoned");
        inner.config.live_path = Some(path);
        inner.config.wallpaper_mode = WallpaperMode::Live;
        inner.paused = false;
        let config = inner.config.clone();
        drop(inner);
        config.save(&state.config_dir).map_err(fail)?;
    }
    state.send(Command::EnterLive);
    Ok(())
}

#[tauri::command]
pub fn stop_live_and_rotate(state: State<'_, Arc<AppState>>) -> Reply<()> {
    {
        let mut inner = state.inner.lock().expect("state poisoned");
        inner.config.wallpaper_mode = WallpaperMode::Rotation;
        inner.paused = false;
        let config = inner.config.clone();
        drop(inner);
        config.save(&state.config_dir).map_err(fail)?;
    }
    state.send(Command::ExitLive);
    Ok(())
}

#[tauri::command]
pub fn enable_gnome_extension(state: State<'_, Arc<AppState>>) -> Reply<&'static str> {
    state
        .backend
        .enable_gnome_extension()
        .map(|status| status.as_str())
        .map_err(fail)
}

#[tauri::command]
pub fn next_wallpaper(state: State<'_, Arc<AppState>>) {
    state.send(Command::Next);
}

#[tauri::command]
pub fn previous_wallpaper(state: State<'_, Arc<AppState>>) {
    state.send(Command::Prev);
}

#[tauri::command]
pub fn toggle_pause(state: State<'_, Arc<AppState>>) {
    state.send(Command::TogglePause);
}

#[tauri::command]
pub fn toggle_exclude(state: State<'_, Arc<AppState>>, path: PathBuf) -> Reply<bool> {
    let (excluded, config) = {
        let mut inner = state.inner.lock().expect("state poisoned");
        let excluded = if let Some(index) = inner
            .config
            .excluded
            .iter()
            .position(|entry| entry == &path)
        {
            inner.config.excluded.remove(index);
            false
        } else {
            inner.config.excluded.push(path);
            true
        };
        (excluded, inner.config.clone())
    };

    config.save(&state.config_dir).map_err(fail)?;
    state.send(Command::Reload);
    Ok(excluded)
}

#[tauri::command]
pub fn get_environment(app: AppHandle, state: State<'_, Arc<AppState>>) -> Environment {
    Environment {
        os: std::env::consts::OS.to_string(),
        backend: state.backend.name(),
        monitor_count: state.backend.monitor_count(),
        supports_per_monitor: state.backend.supports_per_monitor(),
        supports_live: state.backend.live_support() != LiveSupport::None,
        live_support: state.backend.live_support().as_str(),
        gnome_extension_status: state.backend.gnome_extension_status().as_str(),
        app_version: app.package_info().version.to_string(),
        config_path: state
            .config_dir
            .join(config::CONFIG_FILE)
            .to_string_lossy()
            .into_owned(),
        log_path: state.log_dir.to_string_lossy().into_owned(),
        cache_path: state.cache_dir.to_string_lossy().into_owned(),
    }
}

/// Tail of the log file, surfaced in the About screen.
#[tauri::command]
pub fn read_logs(state: State<'_, Arc<AppState>>) -> Reply<String> {
    const MAX_LINES: usize = 500;

    let path = state.log_dir.join(format!("{}.log", crate::LOG_FILE_STEM));
    let contents = std::fs::read_to_string(&path)
        .map_err(|err| format!("could not read {}: {err}", path.display()))?;

    let lines: Vec<&str> = contents.lines().collect();
    let start = lines.len().saturating_sub(MAX_LINES);
    Ok(lines[start..].join("\n"))
}

/// Fetches a remote version.json (or GitHub-style release) outside the webview so
/// CSP / CORS never block the update check.
#[tauri::command]
pub async fn fetch_latest_version(url: String) -> Reply<Option<String>> {
    let parsed = reqwest::Url::parse(&url).map_err(fail)?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err("only http(s) URLs are allowed".into());
    }

    let response = reqwest::Client::new()
        .get(parsed)
        .header(reqwest::header::ACCEPT, "application/json")
        .timeout(std::time::Duration::from_secs(8))
        .send()
        .await
        .map_err(fail)?;

    if !response.status().is_success() {
        return Ok(None);
    }

    let data: serde_json::Value = response.json().await.map_err(fail)?;
    let raw = data
        .get("version")
        .or_else(|| data.get("tag_name"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.strip_prefix('v').unwrap_or(value).to_string());

    Ok(raw)
}

#[tauri::command]
pub async fn search_gallery(
    query: Option<String>,
    ratios: Option<String>,
    categories: Option<String>,
    sorting: Option<String>,
    page: Option<u32>,
    atleast: Option<String>,
) -> Reply<GalleryPage> {
    gallery::search(SearchParams {
        query,
        ratios,
        categories,
        sorting,
        page: page.unwrap_or(1),
        atleast,
    })
    .await
    .map_err(fail)
}

#[tauri::command]
pub async fn add_remote_image(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    url: String,
    expected_size: Option<u64>,
) -> Reply<AddRemoteResult> {
    let saved = gallery::download_and_save(&url, &state.images_dir, expected_size, |progress| {
        if let Err(err) = app.emit(gallery::DOWNLOAD_PROGRESS_EVENT, &progress) {
            log::debug!("no window to receive download progress: {err}");
        }
    })
    .await
    .map_err(fail)?;
    state.send(Command::Reload);
    Ok(AddRemoteResult {
        added: saved.added,
        path: saved.path,
    })
}

fn persist_and_reload(
    app: &AppHandle,
    state: &State<'_, Arc<AppState>>,
    config: Config,
) -> Reply<Config> {
    config.save(&state.config_dir).map_err(fail)?;
    apply_autostart(app, config.autostart);
    watcher::rewatch(state);
    state.send(Command::Reload);
    Ok(config)
}

/// Keeps the OS login item in sync with the checkbox. A failure here is worth
/// logging but should never block saving the rest of the settings.
pub fn apply_autostart(app: &AppHandle, enabled: bool) {
    let manager = app.autolaunch();

    // Enabling is re-applied even when already on, because the login item stores an
    // absolute path to the executable that goes stale after a move or reinstall.
    let result = if enabled {
        manager.enable()
    } else if manager.is_enabled().unwrap_or(false) {
        manager.disable()
    } else {
        Ok(())
    };

    if let Err(err) = result {
        log::warn!("could not update the login item: {err}");
    }
}

struct SavedImage {
    #[allow(dead_code)]
    path: PathBuf,
    added: bool,
}

fn copy_images(images_dir: &Path, paths: &[PathBuf]) -> Reply<usize> {
    let mut added = 0;
    let mut recognised = 0;

    for path in paths {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) => {
                log::warn!("could not read {}: {err}", path.display());
                continue;
            }
        };
        match save_image_bytes(images_dir, &bytes) {
            Ok(saved) => {
                recognised += 1;
                if saved.added {
                    added += 1;
                }
            }
            Err(err) => log::warn!("{}: {err}", path.display()),
        }
    }

    if !paths.is_empty() && recognised == 0 {
        return Err(
            "None of the selected files are supported images (JPG, PNG, WebP or BMP).".into(),
        );
    }

    Ok(added)
}

fn save_image_bytes(images_dir: &Path, bytes: &[u8]) -> Reply<SavedImage> {
    let Some(ext) = scanner::extension_for_bytes(bytes) else {
        return Err("That file is not a supported image (JPG, PNG, WebP or BMP).".into());
    };

    let dest = images_dir.join(format!("{}.{ext}", sha256_hex(bytes)));
    if dest.exists() {
        return Ok(SavedImage {
            path: dest,
            added: false,
        });
    }
    std::fs::write(&dest, bytes).map_err(|err| format!("could not save the image: {err}"))?;
    Ok(SavedImage {
        path: dest,
        added: true,
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write;

    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

fn copy_videos(videos_dir: &Path, paths: &[PathBuf]) -> Reply<Vec<PathBuf>> {
    std::fs::create_dir_all(videos_dir)
        .map_err(|err| format!("could not create {}: {err}", videos_dir.display()))?;

    let mut copied = Vec::new();
    let mut recognised = 0;

    for path in paths {
        match copy_one_video(videos_dir, path) {
            Ok(dest) => {
                recognised += 1;
                copied.push(dest);
            }
            Err(err) => log::warn!("{}: {err}", path.display()),
        }
    }

    if !paths.is_empty() && recognised == 0 {
        return Err("None of the selected files are supported videos (MP4, MOV or M4V).".into());
    }

    Ok(copied)
}

fn copy_one_video(videos_dir: &Path, source: &Path) -> Reply<PathBuf> {
    use sha2::{Digest, Sha256};
    use std::io::{Read, Seek, SeekFrom, Write};

    let mut file = std::fs::File::open(source)
        .map_err(|err| format!("could not read {}: {err}", source.display()))?;
    let mut header = [0u8; 12];
    let read = file
        .read(&mut header)
        .map_err(|err| format!("could not read {}: {err}", source.display()))?;
    let ext = scanner::video_extension_for_bytes(&header[..read])
        .or_else(|| {
            source
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.to_ascii_lowercase())
                .filter(|ext| scanner::VIDEO_EXTENSIONS.contains(&ext.as_str()))
                .map(|ext| match ext.as_str() {
                    "m4v" => "m4v",
                    "mov" => "mov",
                    _ => "mp4",
                })
        })
        .ok_or_else(|| "That file is not a supported video (MP4, MOV or M4V).".to_string())?;

    file.seek(SeekFrom::Start(0))
        .map_err(|err| format!("could not read {}: {err}", source.display()))?;

    let tmp = videos_dir.join(format!(".incoming-{}", std::process::id()));
    let mut hasher = Sha256::new();
    {
        let mut writer = std::fs::File::create(&tmp)
            .map_err(|err| format!("could not save the video: {err}"))?;
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = file
                .read(&mut buf)
                .map_err(|err| format!("could not read {}: {err}", source.display()))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            writer
                .write_all(&buf[..n])
                .map_err(|err| format!("could not save the video: {err}"))?;
        }
    }

    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(hex, "{byte:02x}");
    }
    let dest = videos_dir.join(format!("{hex}.{ext}"));
    if dest.exists() {
        let _ = std::fs::remove_file(&tmp);
        return Ok(dest);
    }
    std::fs::rename(&tmp, &dest).map_err(|err| format!("could not save the video: {err}"))?;
    Ok(dest)
}
