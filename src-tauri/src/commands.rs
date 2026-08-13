use crate::config::{self, Config, Folder};
use crate::scanner;
use crate::state::{AppState, Command, Status};
use crate::{thumbs, watcher};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, State};
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
            path: path.clone(),
        })
        .collect()
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
