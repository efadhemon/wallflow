use super::live::{GnomeExtensionStatus, LiveOpts};
use anyhow::{anyhow, bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const UUID: &str = "live-wallpaper@com.efadhemon.wallflow";
const BUS_NAME: &str = "com.efadhemon.Wallflow.Live";
const OBJECT_PATH: &str = "/com/efadhemon/Wallflow/Live";
const IFACE: &str = "com.efadhemon.Wallflow.Live";

const METADATA: &str = include_str!("../../gnome-extension/metadata.json");
const EXTENSION_JS: &str = include_str!("../../gnome-extension/extension.js");

pub fn is_gnome_shell() -> bool {
    let desktop = [
        "XDG_CURRENT_DESKTOP",
        "XDG_SESSION_DESKTOP",
        "DESKTOP_SESSION",
    ]
    .iter()
    .find_map(|key| std::env::var(key).ok())
    .unwrap_or_default()
    .to_ascii_lowercase();

    if desktop.contains("budgie") || desktop.contains("pantheon") || desktop.contains("cinnamon") {
        return false;
    }
    desktop.contains("gnome") || desktop.contains("ubuntu")
}

pub fn is_wayland() -> bool {
    std::env::var("XDG_SESSION_TYPE")
        .map(|value| value.eq_ignore_ascii_case("wayland"))
        .unwrap_or_else(|_| std::env::var_os("WAYLAND_DISPLAY").is_some())
}

pub fn status() -> GnomeExtensionStatus {
    if !is_gnome_shell() {
        return GnomeExtensionStatus::NotApplicable;
    }
    if ping() {
        return GnomeExtensionStatus::Active;
    }
    if extension_js_error().is_some() {
        return GnomeExtensionStatus::Failed;
    }
    if extension_dir().is_dir() {
        GnomeExtensionStatus::NeedsLogout
    } else {
        GnomeExtensionStatus::NotInstalled
    }
}

pub fn install_and_enable() -> Result<GnomeExtensionStatus> {
    let dir = extension_dir();
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    fs::write(dir.join("metadata.json"), METADATA).context("writing metadata.json")?;
    fs::write(dir.join("extension.js"), EXTENSION_JS).context("writing extension.js")?;

    let _ = Command::new("gsettings")
        .args(["set", "org.gnome.shell", "disable-user-extensions", "false"])
        .status();

    pack_and_install();

    if ping() {
        return Ok(GnomeExtensionStatus::Active);
    }

    // GNOME Wayland only scans ~/.local/share/gnome-shell/extensions at login.
    // Enable fails with "does not exist" until then — that is expected, not a bug.
    if !shell_knows_extension() {
        log::info!(
            "GNOME Shell has not loaded {UUID} yet; log out once so it can enable the live wallpaper extension"
        );
        return Ok(GnomeExtensionStatus::NeedsLogout);
    }

    enable_extension();
    if ping() {
        return Ok(GnomeExtensionStatus::Active);
    }

    if let Some(error) = extension_js_error() {
        bail!("The GNOME live wallpaper extension failed to start: {error}");
    }

    ensure_enabled_in_gsettings();
    Ok(GnomeExtensionStatus::NeedsLogout)
}

fn extension_js_error() -> Option<String> {
    let output = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.gnome.Shell.Extensions",
            "--object-path",
            "/org/gnome/Shell/Extensions",
            "--method",
            "org.gnome.Shell.Extensions.GetExtensionErrors",
            UUID,
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    if trimmed.contains("@as []") || trimmed == "(@as [],)" {
        return None;
    }
    Some(trimmed.to_string())
}

fn pack_and_install() {
    let Some(tmp) = std::env::temp_dir().to_str().map(ToOwned::to_owned) else {
        return;
    };
    let zip = format!("{tmp}/{UUID}.shell-extension.zip");
    let pack = Command::new("gnome-extensions")
        .current_dir(extension_dir())
        .args(["pack", "--force", "-o", &tmp])
        .output();
    match pack {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            log::debug!(
                "gnome-extensions pack: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        Err(_) => return,
    }
    let _ = Command::new("gnome-extensions")
        .args(["install", "--force", &zip])
        .status();
}

fn shell_knows_extension() -> bool {
    Command::new("gnome-extensions")
        .args(["info", UUID])
        .output()
        .is_ok_and(|out| out.status.success())
}

fn ensure_enabled_in_gsettings() {
    let output = Command::new("gsettings")
        .args(["get", "org.gnome.shell", "enabled-extensions"])
        .output();
    let Ok(output) = output else {
        return;
    };
    let current = String::from_utf8_lossy(&output.stdout);
    if current.contains(UUID) {
        return;
    }
    let next = if current.contains(']') {
        current.replace(']', &format!(", '{UUID}']"))
    } else {
        format!("['{UUID}']")
    };
    let _ = Command::new("gsettings")
        .args(["set", "org.gnome.shell", "enabled-extensions", next.trim()])
        .status();
}

fn enable_extension() {
    ensure_enabled_in_gsettings();
    let _ = Command::new("gnome-extensions")
        .args(["disable", UUID])
        .status();
    let enable = Command::new("gnome-extensions")
        .args(["enable", UUID])
        .output();
    match enable {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            log::debug!(
                "gnome-extensions enable: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Err(err) => log::debug!("gnome-extensions is not available: {err}"),
    }
}

pub fn ping() -> bool {
    gdbus(&["Ping"]).is_ok_and(|out| out.contains("true"))
}

pub fn set_video(path: &Path, opts: LiveOpts) -> Result<()> {
    if !ping() {
        let status = install_and_enable()?;
        if status != GnomeExtensionStatus::Active {
            if let Some(error) = extension_js_error() {
                bail!("The GNOME live wallpaper extension failed to start: {error}");
            }
            bail!(
                "The GNOME live wallpaper extension is installed but not running yet. Log out and back in, then try again."
            );
        }
    }
    let fill = format!("{:?}", opts.fill_mode).to_lowercase();
    gdbus(&[
        "SetVideo",
        &gvariant_string(&path.to_string_lossy()),
        &gvariant_string(&fill),
        gvariant_bool(opts.muted),
    ])
    .map(|_| ())
}

pub fn stop() {
    let _ = gdbus(&["Stop"]);
}

pub fn pause(paused: bool) {
    let _ = gdbus(&["Pause", gvariant_bool(paused)]);
}

/// `gdbus call` takes GVariant text, not dbus-send's `boolean:true` form.
fn gvariant_string(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn gvariant_bool(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

fn extension_dir() -> PathBuf {
    let home = std::env::var_os("HOME").unwrap_or_default();
    PathBuf::from(home)
        .join(".local/share/gnome-shell/extensions")
        .join(UUID)
}

fn gdbus(args: &[&str]) -> Result<String> {
    let mut command = Command::new("gdbus");
    command.args([
        "call",
        "--session",
        "--dest",
        BUS_NAME,
        "--object-path",
        OBJECT_PATH,
        "--method",
        &format!("{IFACE}.{}", args[0]),
    ]);
    command.args(&args[1..]);
    let output = command
        .output()
        .with_context(|| anyhow!("running gdbus (is GNOME running?)"))?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
