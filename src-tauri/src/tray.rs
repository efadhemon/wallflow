use crate::state::{AppState, Command, Status};
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

pub const TRAY_ID: &str = "main";
pub const MAIN_WINDOW: &str = "main";

/// Handles to the two menu entries whose text changes at runtime.
pub struct TrayItems {
    current: MenuItem<Wry>,
    pause: MenuItem<Wry>,
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let current = MenuItem::with_id(app, "current", "Starting up...", false, None::<&str>)?;
    let next = MenuItem::with_id(app, "next", "Next wallpaper", true, None::<&str>)?;
    let previous = MenuItem::with_id(app, "previous", "Previous wallpaper", true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause", "Pause", true, None::<&str>)?;
    let show = MenuItem::with_id(app, "show", "Open Wallflow", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &current,
            &PredefinedMenuItem::separator(app)?,
            &next,
            &previous,
            &pause,
            &PredefinedMenuItem::separator(app)?,
            &show,
            &quit,
        ],
    )?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(
            app.default_window_icon()
                .cloned()
                .ok_or_else(|| tauri::Error::AssetNotFound("default window icon".to_string()))?,
        )
        .tooltip("Wallflow")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu_event)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_window(tray.app_handle());
            }
        })
        .build(app)?;

    app.manage(TrayItems { current, pause });
    Ok(())
}

fn on_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    let Some(state) = app.try_state::<Arc<AppState>>() else {
        return;
    };

    match event.id.as_ref() {
        "next" => state.send(Command::Next),
        "previous" => state.send(Command::Prev),
        "pause" => state.send(Command::TogglePause),
        "show" => show_window(app),
        "quit" => app.exit(0),
        _ => {}
    }
}

pub fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub fn update(app: &AppHandle, status: &Status) {
    let label = status
        .current
        .as_ref()
        .map(|image| image.name.clone())
        .unwrap_or_else(|| "No wallpaper set".to_string());

    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(&format!("Wallflow - {label}")));
    }

    if let Some(items) = app.try_state::<TrayItems>() {
        let _ = items.current.set_text(ellipsize(&label, 44));
        let _ = items
            .pause
            .set_text(if status.paused { "Resume" } else { "Pause" });
    }
}

fn ellipsize(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let kept: String = value.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{kept}\u{2026}")
}
