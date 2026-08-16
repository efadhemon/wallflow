# Wallflow

A small desktop app that rotates your wallpaper through folders of your own images,
on a schedule you choose. It lives in the system tray, starts with your session, and
runs on Linux, Windows and macOS.

Built with [Tauri 2](https://v2.tauri.app) — the installers are a few megabytes and
there is no bundled browser runtime.

## Features

- Point it at one or more folders; JPG, JPEG, PNG, WebP and BMP files are picked up.
- Add individual images; they are copied into the app’s data directory and join the rotation.
- Choose the interval in seconds, minutes or hours.
- Shuffle (every image once before repeating), sequential, or plain random order.
- Fill, fit, stretch, center or tile, mapped to whatever your desktop calls it.
- Browse the whole library as thumbnails; click one to apply it immediately, or
  exclude images you would rather not see.
- Skip forwards and backwards, or pause, from the window or the tray menu.
- New images dropped into a watched folder join the rotation without a restart.
- A different image per monitor on Windows and macOS.
- Starts at login and stays out of the way in the tray.

## Supported systems

| System          | Requirement                                       |
| --------------- | ------------------------------------------------- |
| Ubuntu / Debian | 24.04 LTS or newer (needs `libwebkit2gtk-4.1-0`)  |
| Other Linux     | Any distribution, via the AppImage                |
| Windows         | Windows 10 (1803) or newer                        |
| macOS           | 10.15 Catalina or newer (Apple Silicon and Intel) |

On Linux the wallpaper is applied through whatever your desktop provides:

| Desktop                        | Mechanism                                                                      |
| ------------------------------ | ------------------------------------------------------------------------------ |
| GNOME, Unity, Budgie, Pantheon | `gsettings` on `org.gnome.desktop.background`                                  |
| KDE Plasma                     | `qdbus` script on `plasmashell`, falling back to `plasma-apply-wallpaperimage` |
| Xfce                           | `xfconf-query`, applied to every monitor it lists                              |
| Cinnamon                       | `gsettings` on `org.cinnamon.desktop.background`                               |
| MATE                           | `gsettings` on `org.mate.background`                                           |
| LXQt                           | `pcmanfm-qt`                                                                   |
| Anything else                  | Tries gsettings, then qdbus, then `feh`                                        |

Windows uses the `IDesktopWallpaper` COM interface, which is what makes per-monitor
wallpapers possible, and falls back to `SystemParametersInfoW` if that is unavailable.

macOS uses `NSWorkspace setDesktopImageURL:forScreen:options:`, which also supports
a different image per display.

## Install

Grab the file for your system from the
[latest release](../../releases/latest).

**Ubuntu and other Debian-based systems**

```bash
sudo apt install ./Wallflow_<version>_amd64.deb
```

**Any other Linux distribution**

```bash
chmod +x Wallflow_<version>_amd64.AppImage
./Wallflow_<version>_amd64.AppImage
```

**Windows**

Run `Wallflow_<version>_x64-setup.exe`. It installs for the current user only, so no
administrator prompt appears. The installer is not code-signed yet, so SmartScreen
will ask you to confirm via _More info_ to _Run anyway_.

**macOS**

Open `Wallflow_<version>_aarch64.dmg` on Apple Silicon, or `Wallflow_<version>_x64.dmg` on
Intel, and drag Wallflow into Applications. The disk image is not notarized, so the
first launch is blocked by Gatekeeper: right-click the app, choose _Open_, then
confirm _Open_ again.

## Where things are stored

|            | Linux                                                     | Windows                                              | macOS                                                              |
| ---------- | --------------------------------------------------------- | ---------------------------------------------------- | ------------------------------------------------------------------ |
| Settings   | `~/.config/com.efadhemon.wallflow/config.json`            | `%APPDATA%\com.efadhemon.wallflow\config.json`       | `~/Library/Application Support/com.efadhemon.wallflow/config.json` |
| Log        | `~/.local/share/com.efadhemon.wallflow/logs/wallflow.log` | `%APPDATA%\com.efadhemon.wallflow\logs\wallflow.log` | `~/Library/Logs/com.efadhemon.wallflow/wallflow.log`               |
| Thumbnails | `~/.cache/com.efadhemon.wallflow/`                        | `%LOCALAPPDATA%\com.efadhemon.wallflow\`             | `~/Library/Caches/com.efadhemon.wallflow/`                         |

The About screen links to all three and shows the tail of the log.

## Building from source

Requires [Rust](https://rustup.rs) 1.85+, Node.js 22+ and [Yarn](https://classic.yarnpkg.com) 1.x.

On Ubuntu, install the build dependencies first:

```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
  librsvg2-dev libxdo-dev libssl-dev build-essential pkg-config patchelf
```

On macOS, install the Xcode Command Line Tools (`xcode-select --install`).

Then:

```bash
yarn install
yarn tauri dev     # run with hot reload
yarn tauri build   # produce installers in src-tauri/target/release/bundle
```

Releases are built by [.github/workflows/release.yml](.github/workflows/release.yml)
when a `v*` tag is pushed. Linux artifacts are built on Ubuntu 22.04 on purpose: it
is the oldest base that ships WebKitGTK 4.1, which keeps the glibc requirement low
enough to run on 24.04 and 26.04. macOS artifacts are unsigned disk images for
Apple Silicon (`aarch64-apple-darwin`) and Intel (`x86_64-apple-darwin`).

## Notes and limitations

- WebP images are transcoded to PNG in the thumbnail cache before being applied,
  because Windows cannot render WebP wallpapers and GNOME only can with
  `webp-pixbuf-loader` installed.
- Closing the window hides it. Use _Quit_ in the tray menu to stop the app.
- Per-monitor wallpapers are available on Windows and macOS; the Linux backends
  set every monitor to the same image.
- macOS has no wallpaper tile mode, so _Tile_ is applied as _Center_.
- Xfce only exposes its wallpaper properties after the Desktop Settings dialog has
  been opened once on a fresh install.

## License

MIT — see [LICENSE](LICENSE).
