//! Talking to the GNOME Shell shim (`engine/gnome-shim`), which starts the engine
//! as a trusted Wayland client and places its window behind the desktop.

use crate::wallpaper::linux::run;
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

pub const UUID: &str = "wallflow-live@com.efadhemon.wallflow";
/// The old in-shell player from the `live-wall` branch; it decoded video inside
/// gnome-shell and must not run next to the shim.
const OLD_UUID: &str = "live-wallpaper@com.efadhemon.wallflow";

const BUS_NAME: &str = "com.efadhemon.Wallflow.LiveShim";
const OBJECT_PATH: &str = "/com/efadhemon/Wallflow/LiveShim";

/// The shim is compiled in so the app can install it without a resource bundle.
const EXTENSION_JS: &str = include_str!("../../../engine/gnome-shim/extension.js");
const METADATA_JSON: &str = include_str!("../../../engine/gnome-shim/metadata.json");

#[derive(Debug, Clone, Copy, Default)]
pub struct ShimState {
    pub running: bool,
    pub paused: bool,
}

/// The shim answers on the session bus, i.e. it is installed and loaded.
pub fn state() -> Option<ShimState> {
    let out = gdbus(&[
        "--method",
        "org.freedesktop.DBus.Properties.GetAll",
        BUS_NAME,
    ])
    .ok()?;
    Some(ShimState {
        running: out.contains("'Running': <true>"),
        paused: out.contains("'Paused': <true>"),
    })
}

pub fn start(argv: &[String]) -> Result<()> {
    let list = argv
        .iter()
        .map(|arg| gvariant_string(arg))
        .collect::<Vec<_>>()
        .join(", ");
    gdbus(&["--method", &method("Start"), &format!("[{list}]")])
        .context("the GNOME helper could not start the live wallpaper")?;
    Ok(())
}

pub fn stop() -> Result<()> {
    gdbus(&["--method", &method("Stop")]).map(|_| ())
}

pub fn send(line: &str) -> Result<()> {
    gdbus(&["--method", &method("Send"), &gvariant_string(line)]).map(|_| ())
}

fn method(name: &str) -> String {
    format!("{BUS_NAME}.{name}")
}

fn gdbus(args: &[&str]) -> Result<String> {
    let mut all = vec![
        "call",
        "--session",
        "--dest",
        BUS_NAME,
        "--object-path",
        OBJECT_PATH,
    ];
    all.extend_from_slice(args);
    run("gdbus", &all)
}

/// A GVariant text-format string literal: single quotes, with `\` and `'` escaped.
fn gvariant_string(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn extensions_dir() -> Result<PathBuf> {
    let data = match std::env::var_os("XDG_DATA_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => {
            PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?).join(".local/share")
        }
    };
    Ok(data.join("gnome-shell/extensions"))
}

/// Installed for this user, whether or not GNOME has loaded it yet.
pub fn installed() -> bool {
    extensions_dir().is_ok_and(|dir| dir.join(UUID).join("metadata.json").is_file())
}

/// Copies the shim into the user's extensions and enables it (and turns off the
/// old in-shell player). GNOME on Wayland only loads it at the next login.
pub fn install() -> Result<()> {
    let dir = extensions_dir()?.join(UUID);
    // A development symlink (engine/gnome-shim/install.sh) is left alone.
    if !dir.is_symlink() {
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        write(&dir.join("extension.js"), EXTENSION_JS)?;
        write(&dir.join("metadata.json"), METADATA_JSON)?;
    }

    let raw = run(
        "gsettings",
        &["get", "org.gnome.shell", "enabled-extensions"],
    )?;
    let enabled = edit_enabled(&raw, UUID, OLD_UUID);
    run(
        "gsettings",
        &["set", "org.gnome.shell", "enabled-extensions", &enabled],
    )?;
    // Allowed to fail: GNOME may not know the new extension until the next login.
    let _ = run("gnome-extensions", &["disable", OLD_UUID]);
    let _ = run("gnome-extensions", &["enable", UUID]);
    Ok(())
}

fn write(path: &Path, contents: &str) -> Result<()> {
    std::fs::write(path, contents).with_context(|| format!("writing {}", path.display()))
}

/// `enabled-extensions` with `add` appended and `remove` dropped, keeping the
/// user's other extensions as they are.
fn edit_enabled(raw: &str, add: &str, remove: &str) -> String {
    let inner = raw
        .trim()
        .trim_start_matches("@as")
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']');
    let mut items: Vec<String> = inner
        .split(',')
        .map(|item| item.trim().trim_matches('\'').to_string())
        .filter(|item| !item.is_empty() && item != remove)
        .collect();
    if !items.iter().any(|item| item == add) {
        items.push(add.to_string());
    }
    let quoted: Vec<String> = items.iter().map(|item| format!("'{item}'")).collect();
    format!("[{}]", quoted.join(", "))
}

pub fn session_supported() -> std::result::Result<(), String> {
    use crate::wallpaper::linux::{detect_desktop, DesktopEnv};
    if detect_desktop() != DesktopEnv::Gnome {
        return Err(
            "Live wallpapers currently need GNOME. KDE, Xfce and others are planned.".into(),
        );
    }
    let wayland = std::env::var("XDG_SESSION_TYPE").is_ok_and(|kind| kind == "wayland")
        || std::env::var_os("WAYLAND_DISPLAY").is_some();
    if !wayland {
        return Err("Live wallpapers on GNOME need a Wayland session.".into());
    }
    Ok(())
}

pub fn ensure_ready() -> Result<()> {
    if let Err(reason) = session_supported() {
        bail!(reason);
    }
    if state().is_none() {
        if installed() {
            bail!("Log out and back in to finish setting up live wallpapers.");
        }
        bail!("Set up the GNOME helper on the Live page first.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_are_escaped_for_gvariant() {
        assert_eq!(gvariant_string("/a/it's \\ here"), r"'/a/it\'s \\ here'");
    }

    #[test]
    fn enabled_list_keeps_others_adds_ours_and_drops_the_old_player() {
        assert_eq!(
            edit_enabled(
                "['ding@rastersoft.com', 'live-wallpaper@com.efadhemon.wallflow']",
                UUID,
                OLD_UUID
            ),
            format!("['ding@rastersoft.com', '{UUID}']")
        );
        assert_eq!(
            edit_enabled("@as []", UUID, OLD_UUID),
            format!("['{UUID}']")
        );
        // Already enabled: unchanged.
        assert_eq!(
            edit_enabled(&format!("['{UUID}']"), UUID, OLD_UUID),
            format!("['{UUID}']")
        );
    }
}
