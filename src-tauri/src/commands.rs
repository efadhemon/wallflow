use crate::config::{self, Config, Folder};
use crate::gallery::{self, GalleryPage, SearchParams};
use crate::live::{self, LiveStatus, LiveVideo};
use crate::scanner;
use crate::state::{AppState, Command, Status};
use crate::{scheduler, thumbs, watcher};
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

/// Persists the whole settings object, then tells the scheduler only what it
/// needs to redo: a full rebuild for playlist settings, a re-apply for how the
/// image is drawn, or just a new countdown for the interval.
#[tauri::command]
pub async fn set_config(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    mut config: Config,
) -> Reply<Config> {
    config.sanitize();

    let (folders_changed, playlist_changed, look_changed, interval_changed, autostart_changed) = {
        let mut inner = state.inner.lock().expect("state poisoned");
        // Which live wallpaper plays is the backend's business; a settings form
        // holding an older copy must not start or stop one.
        config.live_video = inner.config.live_video.clone();
        // Saved under the lock so two quick changes are written in the order they win.
        config.save(&state.config_dir).map_err(fail)?;
        let old = &inner.config;
        let folders_changed = old.folders != config.folders;
        let changes = (
            folders_changed,
            folders_changed || old.order != config.order || old.excluded != config.excluded,
            old.fill_mode != config.fill_mode || old.per_monitor != config.per_monitor,
            old.interval_seconds != config.interval_seconds,
            old.autostart != config.autostart,
        );
        inner.config = config.clone();
        changes
    };

    if autostart_changed {
        apply_autostart(&app, config.autostart);
    }
    if folders_changed {
        watcher::rewatch(&state);
    }
    if playlist_changed {
        state.send(Command::Reload);
    } else if interval_changed {
        state.send(Command::Rearm);
    }
    if look_changed {
        if config.live_video.is_some() {
            // Re-applying an image would end the live wallpaper; tell the engine.
            let fill = config.fill_mode;
            tokio::task::spawn_blocking(move || live::set_fill(fill));
        } else {
            state.send(Command::ApplyCurrent);
        }
    }

    Ok(config)
}

#[tauri::command]
pub async fn add_folder(
    state: State<'_, Arc<AppState>>,
    path: PathBuf,
    recursive: bool,
) -> Reply<Config> {
    if !path.is_dir() {
        return Err(format!("{} is not a folder", path.display()));
    }

    update_folders(&state, |folders| {
        if folders.iter().any(|folder| folder.path == path) {
            return Err("That folder is already in the list.".to_string());
        }
        folders.push(Folder { path, recursive });
        Ok(())
    })
}

#[tauri::command]
pub async fn remove_folder(state: State<'_, Arc<AppState>>, path: PathBuf) -> Reply<Config> {
    update_folders(&state, |folders| {
        folders.retain(|folder| folder.path != path);
        Ok(())
    })
}

#[tauri::command]
pub async fn set_folder_recursive(
    state: State<'_, Arc<AppState>>,
    path: PathBuf,
    recursive: bool,
) -> Reply<Config> {
    update_folders(&state, |folders| {
        if let Some(folder) = folders.iter_mut().find(|f| f.path == path) {
            folder.recursive = recursive;
        }
        Ok(())
    })
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
    state.send(Command::Refresh);
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

    {
        let mut inner = state.inner.lock().expect("state poisoned");
        inner
            .config
            .excluded
            .retain(|entry| entry != &path && entry != &canonical);
        inner.config.save(&state.config_dir).map_err(fail)?;
    }
    state.send(Command::Refresh);
    Ok(())
}

/// Decoding happens off the async runtime; the UI asks for these one tile at a time.
#[tauri::command]
pub async fn get_thumbnail(state: State<'_, Arc<AppState>>, path: PathBuf) -> Reply<String> {
    let cache_dir = state.cache_dir.clone();

    tokio::task::spawn_blocking(move || thumbs::thumbnail(&cache_dir, &path))
        .await
        .map_err(fail)?
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(fail)
}

/// Screen-sized copy for the full-screen preview. Library images can live anywhere
/// on disk, so they are re-encoded into the cache rather than exposed directly.
#[tauri::command]
pub async fn get_preview(state: State<'_, Arc<AppState>>, path: PathBuf) -> Reply<String> {
    let cache_dir = state.cache_dir.clone();

    tokio::task::spawn_blocking(move || thumbs::preview(&cache_dir, &path))
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
    let excluded = {
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
        inner.config.save(&state.config_dir).map_err(fail)?;
        excluded
    };

    // Not a full reload: the shuffle pass and the countdown carry on.
    state.send(Command::Refresh);
    Ok(excluded)
}

#[tauri::command]
pub fn get_environment(app: AppHandle, state: State<'_, Arc<AppState>>) -> Environment {
    Environment {
        os: std::env::consts::OS.to_string(),
        backend: state.backend.name(),
        monitor_count: state.backend.monitor_count(),
        supports_per_monitor: state.backend.supports_per_monitor(),
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

// Tauri maps each argument to a key of the IPC payload, so they stay flat.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn search_gallery(
    query: Option<String>,
    ratios: Option<String>,
    categories: Option<String>,
    sorting: Option<String>,
    page: Option<u32>,
    atleast: Option<String>,
    top_range: Option<String>,
    colors: Option<String>,
) -> Reply<GalleryPage> {
    gallery::search(SearchParams {
        query,
        ratios,
        categories,
        sorting,
        page: page.unwrap_or(1),
        atleast,
        top_range,
        colors,
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
    state.send(Command::Refresh);
    Ok(AddRemoteResult {
        added: saved.added,
        path: saved.path,
    })
}

/// Applies a change to the folder list, saves it and rescans. The change and the
/// save happen under one lock so concurrent edits cannot overwrite each other.
fn update_folders(
    state: &State<'_, Arc<AppState>>,
    change: impl FnOnce(&mut Vec<Folder>) -> Reply<()>,
) -> Reply<Config> {
    let config = {
        let mut inner = state.inner.lock().expect("state poisoned");
        let mut config = inner.config.clone();
        change(&mut config.folders)?;
        config.sanitize();
        config.save(&state.config_dir).map_err(fail)?;
        inner.config = config.clone();
        config
    };

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
    config::write_atomic(&dest, |temp| {
        std::fs::write(temp, bytes).map_err(Into::into)
    })
    .map_err(|err| format!("could not save the image: {err:#}"))?;
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

#[tauri::command]
pub async fn live_status(state: State<'_, Arc<AppState>>) -> Reply<LiveStatus> {
    let active = state.config().live_video;
    tokio::task::spawn_blocking(move || live::status(active))
        .await
        .map_err(fail)
}

#[tauri::command]
pub fn live_list(state: State<'_, Arc<AppState>>) -> Vec<LiveVideo> {
    state.live.list()
}

/// Prepares the picked videos one after another (each already uses half the
/// cores), reporting progress through `live://import` events.
#[tauri::command]
pub async fn live_import(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    paths: Vec<PathBuf>,
) -> Reply<Vec<LiveVideo>> {
    let engine = live::engine_path().map_err(fail)?;
    if !live::ffmpeg_available() {
        return Err("FFmpeg is needed to add live wallpapers. Install it (e.g. `sudo apt install ffmpeg`) and try again.".into());
    }
    let screen = live::screen_size(&app);
    let emit = |progress: live::ImportProgress| {
        if let Err(err) = app.emit(live::IMPORT_EVENT, &progress) {
            log::debug!("no window to receive import progress: {err}");
        }
    };

    let mut added = Vec::new();
    let mut first_error = None;
    for path in paths {
        match state.live.import(&emit, &engine, &path, screen).await {
            Ok(video) => added.push(video),
            Err(err) => {
                log::warn!("could not add {}: {err:#}", path.display());
                first_error.get_or_insert_with(|| format!("{err:#}"));
            }
        }
    }
    match (added.is_empty(), first_error) {
        (true, Some(err)) => Err(err),
        _ => Ok(added),
    }
}

#[tauri::command]
pub async fn live_remove(app: AppHandle, state: State<'_, Arc<AppState>>, id: String) -> Reply<()> {
    if state.config().live_video.as_deref() == Some(id.as_str()) {
        scheduler::end_live(&state).await;
        state.send(Command::ApplyCurrent);
    }
    state.live.remove(&id).map_err(fail)?;
    scheduler::publish(&app, &state);
    Ok(())
}

/// Plays a library video as the wallpaper. The rotation pauses and the video's
/// poster becomes the static wallpaper (lock screen, before the engine is up).
#[tauri::command]
pub async fn live_play(app: AppHandle, state: State<'_, Arc<AppState>>, id: String) -> Reply<()> {
    let video = state
        .live
        .get(&id)
        .ok_or_else(|| "That live wallpaper is no longer in the library.".to_string())?;
    let fill = state.config().fill_mode;

    let started = video.clone();
    tokio::task::spawn_blocking(move || live::start(&started, fill))
        .await
        .map_err(fail)?
        .map_err(fail)?;

    {
        let mut inner = state.inner.lock().expect("state poisoned");
        inner.config.live_video = Some(id);
        inner.config.save(&state.config_dir).map_err(fail)?;
    }

    let shared = state.inner().clone();
    let poster = video.poster_path;
    tokio::task::spawn_blocking(move || {
        if let Err(err) = shared.backend.set(&poster, fill) {
            log::warn!("could not set the poster as the static wallpaper: {err:#}");
        }
    });

    scheduler::publish(&app, &state);
    Ok(())
}

/// Stops the live wallpaper and puts the rotation's current image back.
#[tauri::command]
pub async fn live_stop(app: AppHandle, state: State<'_, Arc<AppState>>) -> Reply<()> {
    scheduler::end_live(&state).await;
    state.send(Command::ApplyCurrent);
    scheduler::publish(&app, &state);
    Ok(())
}

/// GNOME: installs and enables the helper extension. It loads at the next login.
#[tauri::command]
pub async fn live_install_helper(state: State<'_, Arc<AppState>>) -> Reply<LiveStatus> {
    tokio::task::spawn_blocking(live::install_helper)
        .await
        .map_err(fail)?
        .map_err(fail)?;
    live_status(state).await
}
