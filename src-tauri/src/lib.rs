mod commands;
mod config;
mod gallery;
mod scanner;
mod scheduler;
mod state;
mod thumbs;
mod tray;
mod wallpaper;
mod watcher;

use config::Config;
use state::{AppState, Inner};
use std::sync::{Arc, Mutex};
use tauri::{Manager, RunEvent, WindowEvent};
use tauri_plugin_aptabase::EventTracker;
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_log::{Target, TargetKind};

/// Passed by the login item so a session start goes straight to the tray.
const MINIMIZED_FLAG: &str = "--minimized";

/// Base name of the log file; the log plugin appends the extension.
pub(crate) const LOG_FILE_STEM: &str = "wallflow";

/// Compiled in from the environment so the key is never committed.
const APTABASE_APP_KEY: &str = match option_env!("APTABASE_APP_KEY") {
    Some(key) => key,
    None => "",
};

fn tracking_enabled() -> bool {
    !cfg!(debug_assertions) && !APTABASE_APP_KEY.is_empty()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_window(app);
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .target(Target::new(TargetKind::LogDir {
                    file_name: Some(LOG_FILE_STEM.to_string()),
                }))
                .target(Target::new(TargetKind::Stdout))
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![MINIMIZED_FLAG]),
        ))
        .plugin(tauri_plugin_aptabase::Builder::new(APTABASE_APP_KEY).build())
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::get_config,
            commands::set_config,
            commands::add_folder,
            commands::remove_folder,
            commands::set_folder_recursive,
            commands::list_folders,
            commands::list_images,
            commands::add_images,
            commands::remove_image,
            commands::get_thumbnail,
            commands::apply_image,
            commands::next_wallpaper,
            commands::previous_wallpaper,
            commands::toggle_pause,
            commands::toggle_exclude,
            commands::get_environment,
            commands::read_logs,
            commands::fetch_latest_version,
            commands::search_gallery,
            commands::add_remote_image,
        ])
        .setup(setup)
        .build(tauri::generate_context!())
        .expect("failed to start Wallflow")
        .run(|handler, event| match event {
            RunEvent::Ready => {
                if tracking_enabled() {
                    let _ = handler.track_event("app_started", None);
                }
            }
            RunEvent::Exit => {
                if tracking_enabled() {
                    let _ = handler.track_event("app_exited", None);
                    handler.flush_events_blocking();
                }
            }
            // Closing the window only hides it, so an explicit exit is the one way out.
            RunEvent::ExitRequested {
                code: None, api, ..
            } => {
                api.prevent_exit();
            }
            _ => {}
        });
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();

    let config_dir = app.path().app_config_dir()?;
    let cache_dir = app.path().app_cache_dir()?;
    let log_dir = app.path().app_log_dir()?;
    let images_dir = app.path().app_data_dir()?.join("images");
    for dir in [&config_dir, &cache_dir, &log_dir, &images_dir] {
        std::fs::create_dir_all(dir)?;
    }

    let config = Config::load(&config_dir);
    if !config_dir.join(config::CONFIG_FILE).is_file() {
        config.save(&config_dir)?;
    }
    let start_minimized = config.start_minimized;
    let autostart = config.autostart;
    let theme = config.theme;

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let state = Arc::new(AppState {
        inner: Mutex::new(Inner {
            config,
            images: Vec::new(),
            playlist: Vec::new(),
            index: 0,
            current: None,
            paused: false,
            next_change_at: None,
            last_error: None,
        }),
        backend: wallpaper::detect(),
        config_dir,
        cache_dir: cache_dir.clone(),
        log_dir,
        images_dir,
        tx,
        watcher: Mutex::new(None),
    });
    app.manage(state.clone());

    // Thumbnails are served to the webview straight off disk; nothing outside the
    // cache directory is reachable through the asset protocol.
    app.asset_protocol_scope()
        .allow_directory(&cache_dir, true)?;

    tray::build(&handle)?;
    commands::apply_autostart(&handle, autostart);
    watcher::rewatch(&state);

    if let Some(window) = app.get_webview_window(tray::MAIN_WINDOW) {
        // GNOME draws CSD controls over the webview, so they ignore clicks until a resize.
        // Linux and Windows use an in-page titlebar instead;
        // macOS keeps the native chrome.
        #[cfg(any(target_os = "linux", windows))]
        window.set_decorations(false)?;

        let window_theme = match theme {
            config::Theme::Light => Some(tauri::Theme::Light),
            config::Theme::Dark => Some(tauri::Theme::Dark),
            config::Theme::System => None,
        };
        window.set_theme(window_theme)?;

        let launched_at_login = std::env::args().any(|arg| arg == MINIMIZED_FLAG);
        if !(start_minimized && launched_at_login) {
            window.show()?;
        }

        let hidden = window.clone();
        window.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = hidden.hide();
            }
        });
    }

    tauri::async_runtime::spawn(scheduler::run(handle, rx));
    Ok(())
}
