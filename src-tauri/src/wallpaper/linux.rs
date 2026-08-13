use super::WallpaperBackend;
use crate::config::FillMode;
use anyhow::{anyhow, bail, Context, Result};
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use std::path::Path;
use std::process::Command;

/// Characters that must not appear raw in a `file://` URI. `%` is included so an
/// image literally named `100%.jpg` round-trips instead of producing a broken URI.
const URI_ESCAPE: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'<')
    .add(b'>')
    .add(b'`')
    .add(b'#')
    .add(b'?')
    .add(b'{')
    .add(b'}')
    .add(b'%')
    .add(b'[')
    .add(b']')
    .add(b'\\')
    .add(b'^')
    .add(b'|')
    .add(b'\'');

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopEnv {
    Gnome,
    Kde,
    Xfce,
    Cinnamon,
    Mate,
    Lxqt,
    Unknown,
}

impl DesktopEnv {
    pub fn label(&self) -> &'static str {
        match self {
            DesktopEnv::Gnome => "GNOME",
            DesktopEnv::Kde => "KDE Plasma",
            DesktopEnv::Xfce => "Xfce",
            DesktopEnv::Cinnamon => "Cinnamon",
            DesktopEnv::Mate => "MATE",
            DesktopEnv::Lxqt => "LXQt",
            DesktopEnv::Unknown => "Unknown desktop",
        }
    }
}

pub fn detect_desktop() -> DesktopEnv {
    let raw = [
        "XDG_CURRENT_DESKTOP",
        "XDG_SESSION_DESKTOP",
        "DESKTOP_SESSION",
    ]
    .iter()
    .find_map(|key| std::env::var(key).ok())
    .unwrap_or_default()
    .to_ascii_lowercase();

    if raw.contains("gnome")
        || raw.contains("unity")
        || raw.contains("budgie")
        || raw.contains("pantheon")
    {
        DesktopEnv::Gnome
    } else if raw.contains("kde") || raw.contains("plasma") {
        DesktopEnv::Kde
    } else if raw.contains("xfce") {
        DesktopEnv::Xfce
    } else if raw.contains("cinnamon") {
        DesktopEnv::Cinnamon
    } else if raw.contains("mate") {
        DesktopEnv::Mate
    } else if raw.contains("lxqt") {
        DesktopEnv::Lxqt
    } else {
        DesktopEnv::Unknown
    }
}

pub struct LinuxBackend {
    desktop: DesktopEnv,
}

impl LinuxBackend {
    pub fn new() -> Self {
        let desktop = detect_desktop();
        log::info!("detected desktop environment: {}", desktop.label());
        Self { desktop }
    }
}

impl WallpaperBackend for LinuxBackend {
    fn name(&self) -> String {
        self.desktop.label().to_string()
    }

    fn set(&self, path: &Path, mode: FillMode) -> Result<()> {
        match self.desktop {
            DesktopEnv::Gnome => set_gsettings("org.gnome.desktop.background", path, mode, true),
            DesktopEnv::Kde => set_kde(path, mode),
            DesktopEnv::Xfce => set_xfce(path, mode),
            DesktopEnv::Cinnamon => {
                set_gsettings("org.cinnamon.desktop.background", path, mode, false)
            }
            DesktopEnv::Mate => set_mate(path, mode),
            DesktopEnv::Lxqt => set_lxqt(path, mode),
            DesktopEnv::Unknown => set_unknown(path, mode),
        }
    }
}

fn file_uri(path: &Path) -> String {
    format!(
        "file://{}",
        utf8_percent_encode(&path.to_string_lossy(), URI_ESCAPE)
    )
}

/// D-Bus is inherited from the session in the normal case, but a session started
/// from a bare login shell or a stale autostart entry may be missing it.
fn dbus_address() -> Option<String> {
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some() {
        return None;
    }
    let uid = unsafe { libc_getuid() };
    let socket = format!("/run/user/{uid}/bus");
    Path::new(&socket)
        .exists()
        .then(|| format!("unix:path={socket}"))
}

// Avoids pulling in the `libc` crate for a single call.
unsafe fn libc_getuid() -> u32 {
    extern "C" {
        fn getuid() -> u32;
    }
    getuid()
}

fn run(program: &str, args: &[&str]) -> Result<String> {
    let mut command = Command::new(program);
    command.args(args);
    if let Some(address) = dbus_address() {
        command.env("DBUS_SESSION_BUS_ADDRESS", address);
    }

    let output = command
        .output()
        .with_context(|| format!("running `{program}` (is it installed?)"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!("`{program} {}` failed: {stderr}", args.join(" "));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn which(program: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {program}"))
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// `picture-options` values shared by GNOME, Cinnamon and MATE.
fn gsettings_picture_option(mode: FillMode) -> &'static str {
    match mode {
        FillMode::Fill => "zoom",
        FillMode::Fit => "scaled",
        FillMode::Stretch => "stretched",
        FillMode::Center => "centered",
        FillMode::Tile => "wallpaper",
    }
}

fn set_gsettings(schema: &str, path: &Path, mode: FillMode, dark_variant: bool) -> Result<()> {
    let uri = file_uri(path);
    run("gsettings", &["set", schema, "picture-uri", &uri])?;

    if dark_variant {
        // Missing on GNOME < 42; a failure here must not fail the whole change.
        if let Err(err) = run("gsettings", &["set", schema, "picture-uri-dark", &uri]) {
            log::debug!("picture-uri-dark not available: {err}");
        }
    }

    let option = gsettings_picture_option(mode);
    if let Err(err) = run("gsettings", &["set", schema, "picture-options", option]) {
        log::debug!("could not set picture-options: {err}");
    }

    Ok(())
}

fn set_mate(path: &Path, mode: FillMode) -> Result<()> {
    // MATE takes a plain path rather than a URI.
    let plain = path.to_string_lossy().into_owned();
    run(
        "gsettings",
        &["set", "org.mate.background", "picture-filename", &plain],
    )?;

    let option = gsettings_picture_option(mode);
    if let Err(err) = run(
        "gsettings",
        &["set", "org.mate.background", "picture-options", option],
    ) {
        log::debug!("could not set MATE picture-options: {err}");
    }
    Ok(())
}

/// `org.kde.image` FillMode enum values.
fn kde_fill_mode(mode: FillMode) -> u8 {
    match mode {
        FillMode::Stretch => 0,
        FillMode::Fit => 1,
        FillMode::Fill => 2,
        FillMode::Tile => 3,
        FillMode::Center => 6,
    }
}

fn set_kde(path: &Path, mode: FillMode) -> Result<()> {
    let uri = file_uri(path);
    let fill = kde_fill_mode(mode);
    let script = format!(
        r#"var desks = desktops();
for (var i = 0; i < desks.length; i++) {{
    var d = desks[i];
    d.wallpaperPlugin = "org.kde.image";
    d.currentConfigGroup = Array("Wallpaper", "org.kde.image", "General");
    d.writeConfig("Image", "{uri}");
    d.writeConfig("FillMode", {fill});
}}"#
    );

    let mut last_error = None;
    for qdbus in ["qdbus6", "qdbus-qt6", "qdbus", "qdbus-qt5"] {
        if !which(qdbus) {
            continue;
        }
        match run(
            qdbus,
            &[
                "org.kde.plasmashell",
                "/PlasmaShell",
                "org.kde.PlasmaShell.evaluateScript",
                &script,
            ],
        ) {
            Ok(_) => return Ok(()),
            Err(err) => last_error = Some(err),
        }
    }

    // Plasma 5.21+ ships this, but it cannot configure the fill mode.
    if which("plasma-apply-wallpaperimage") {
        let plain = path.to_string_lossy().into_owned();
        return run("plasma-apply-wallpaperimage", &[&plain]).map(|_| ());
    }

    Err(last_error.unwrap_or_else(|| {
        anyhow!(
            "no KDE wallpaper tool found; install plasma-workspace for plasma-apply-wallpaperimage"
        )
    }))
}

/// Xfce `image-style` enum values.
fn xfce_image_style(mode: FillMode) -> &'static str {
    match mode {
        FillMode::Center => "1",
        FillMode::Tile => "2",
        FillMode::Stretch => "3",
        FillMode::Fit => "4",
        FillMode::Fill => "5",
    }
}

fn set_xfce(path: &Path, mode: FillMode) -> Result<()> {
    let plain = path.to_string_lossy().into_owned();
    let style = xfce_image_style(mode);

    let listed = run("xfconf-query", &["-c", "xfce4-desktop", "-l"])?;
    let properties: Vec<&str> = listed
        .lines()
        .map(str::trim)
        .filter(|line| line.ends_with("/last-image"))
        .collect();

    if properties.is_empty() {
        // Xfce only creates these once the Desktop Settings dialog has been opened.
        let fallback = "/backdrop/screen0/monitor0/workspace0/last-image";
        run(
            "xfconf-query",
            &[
                "-c",
                "xfce4-desktop",
                "-p",
                fallback,
                "-n",
                "-t",
                "string",
                "-s",
                &plain,
            ],
        )?;
        return Ok(());
    }

    for property in properties {
        run(
            "xfconf-query",
            &["-c", "xfce4-desktop", "-p", property, "-s", &plain],
        )?;

        let style_property = property.replace("last-image", "image-style");
        if let Err(err) = run(
            "xfconf-query",
            &[
                "-c",
                "xfce4-desktop",
                "-p",
                &style_property,
                "-n",
                "-t",
                "int",
                "-s",
                style,
            ],
        ) {
            log::debug!("could not set {style_property}: {err}");
        }
    }

    Ok(())
}

fn set_lxqt(path: &Path, mode: FillMode) -> Result<()> {
    let plain = path.to_string_lossy().into_owned();
    let wallpaper_mode = match mode {
        FillMode::Fill => "crop",
        FillMode::Fit => "fit",
        FillMode::Stretch => "stretch",
        FillMode::Center => "center",
        FillMode::Tile => "tile",
    };
    run(
        "pcmanfm-qt",
        &[
            "--set-wallpaper",
            &plain,
            &format!("--wallpaper-mode={wallpaper_mode}"),
        ],
    )
    .map(|_| ())
}

/// Best effort for desktops we do not know by name: try the two config systems
/// most of them are built on, then a plain X11 wallpaper setter.
fn set_unknown(path: &Path, mode: FillMode) -> Result<()> {
    let mut errors = Vec::new();

    for attempt in [
        set_gsettings("org.gnome.desktop.background", path, mode, true),
        set_kde(path, mode),
    ] {
        match attempt {
            Ok(()) => return Ok(()),
            Err(err) => errors.push(err.to_string()),
        }
    }

    if which("feh") {
        let plain = path.to_string_lossy().into_owned();
        let flag = match mode {
            FillMode::Fill => "--bg-fill",
            FillMode::Fit => "--bg-max",
            FillMode::Stretch => "--bg-scale",
            FillMode::Center => "--bg-center",
            FillMode::Tile => "--bg-tile",
        };
        if run("feh", &[flag, &plain]).is_ok() {
            return Ok(());
        }
    }

    bail!(
        "could not detect how to set the wallpaper on this desktop. Set XDG_CURRENT_DESKTOP \
         to one of gnome, kde, xfce, cinnamon, mate or lxqt. Details: {}",
        errors.join("; ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn uri_escapes_spaces_and_percent() {
        assert_eq!(
            file_uri(&PathBuf::from("/home/a/my wall 100%.jpg")),
            "file:///home/a/my%20wall%20100%25.jpg"
        );
    }

    #[test]
    fn uri_keeps_path_separators() {
        assert_eq!(file_uri(&PathBuf::from("/a/b/c.png")), "file:///a/b/c.png");
    }
}
