# Wallflow

A small desktop app that rotates your wallpaper through your own images — folders
you watch, or files you add one by one — on a schedule you choose. It can also play
a video as a live wallpaper. It lives in the system tray, starts with your
session, and runs on Linux, Windows and macOS.

Built with [Tauri 2](https://v2.tauri.app) — the installers are a few megabytes and
there is no bundled browser runtime.

## Features

- Point it at one or more folders; JPG, JPEG, PNG, WebP and BMP files are picked up.
- Add individual images; Wallflow copies them into its data directory so they stay in
  the rotation even if you move the originals. The same file is not copied twice.
  Delete a copy from Library when you no longer want it.
- Choose the interval in seconds, minutes or hours.
- Shuffle (every image once before repeating), sequential, or plain random order.
- Fill, fit, stretch, center or tile, mapped to whatever your desktop calls it.
- Browse the whole library as thumbnails; click one to apply it immediately, or
  exclude images you would rather not see. Filter the gallery to added copies only.
- Search the Gallery for landscape wallpapers from [Wallhaven](https://wallhaven.cc)
  (SFW only, no API key). Open one full screen to look closer, add it to the library,
  or set it as the wallpaper now.
- Play a video as a live wallpaper on GNOME, Windows and macOS (see
  [Live wallpapers](#live-wallpapers)).
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

## Live wallpapers

Open **Live**, add videos (MP4, MKV, WebM, MOV, M4V or AVI) and press **Play**.

- Each video is prepared once when you add it: scaled down to your screen, capped
  at 30 fps, with a poster frame. Playback then needs little work.
- The video runs in its own process, `wallflow-live`, decoded by FFmpeg (hardware
  decoding when available) and drawn with the GPU, in windows placed behind the
  desktop icons. The desktop shell does no video work, so it stays smooth.
- It plays on every monitor and pauses on its own while the desktop is covered
  (a maximized or full-screen window, the overview or Mission Control, the lock
  screen).
- While it plays, the image rotation waits, and the video's first frame is set as
  the static wallpaper (lock screen, login). **Stop**, picking an image in Library,
  or **Next** ends it.

| System           | Live wallpapers                                                        |
| ---------------- | ---------------------------------------------------------------------- |
| GNOME on Wayland | Yes. Needs FFmpeg and a small helper extension (one-time setup, below) |
| Windows 10 / 11  | Yes, new in 1.1.0. FFmpeg ships in the installer, nothing to set up    |
| macOS 11+        | Yes, new in 1.1.0. FFmpeg ships in the app, nothing to set up          |
| KDE, X11, others | Not yet                                                                |

Live wallpapers on Windows and macOS are new: if something looks wrong (the video
over your windows, not behind the icons, or not pausing), please
[open an issue](../../issues) with your system version and the Recent log from
About.

**One-time setup on GNOME.** The first time, the Live page offers **Set up**: it
installs a small GNOME Shell extension (`wallflow-live@com.efadhemon.wallflow`) that
places the video window behind the desktop. It does no video work itself. GNOME on
Wayland loads new extensions only at login, so log out and back in once.

The `.deb` pulls in FFmpeg. With the AppImage, install it yourself
(`sudo apt install ffmpeg` on Ubuntu); the Live page says so if it is missing.

The engine is documented in [engine/README.md](engine/README.md).

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

|              | Linux                                                     | Windows                                              | macOS                                                              |
| ------------ | --------------------------------------------------------- | ---------------------------------------------------- | ------------------------------------------------------------------ |
| Settings     | `~/.config/com.efadhemon.wallflow/config.json`            | `%APPDATA%\com.efadhemon.wallflow\config.json`       | `~/Library/Application Support/com.efadhemon.wallflow/config.json` |
| Added images | `~/.local/share/com.efadhemon.wallflow/images/`           | `%APPDATA%\com.efadhemon.wallflow\images\`           | `~/Library/Application Support/com.efadhemon.wallflow/images/`     |
| Log          | `~/.local/share/com.efadhemon.wallflow/logs/wallflow.log` | `%APPDATA%\com.efadhemon.wallflow\logs\wallflow.log` | `~/Library/Logs/com.efadhemon.wallflow/wallflow.log`               |
| Thumbnails   | `~/.cache/com.efadhemon.wallflow/`                        | `%LOCALAPPDATA%\com.efadhemon.wallflow\`             | `~/Library/Caches/com.efadhemon.wallflow/`                         |
| Live videos  | `~/.local/share/com.efadhemon.wallflow/live/`             | `%APPDATA%\com.efadhemon.wallflow\live\`             | `~/Library/Application Support/com.efadhemon.wallflow/live/`       |

The About screen links to settings, logs and the thumbnail cache. Added images are
copies; deleting one in Library removes Wallflow’s file, not the original. Live
videos are prepared copies too; removing one on the Live page deletes the copy only.

## Building from source

Requires [Rust](https://rustup.rs) 1.85+, Node.js 22+ and [Yarn](https://classic.yarnpkg.com) 1.x.

On Ubuntu, install the build dependencies first:

```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
  librsvg2-dev libxdo-dev libssl-dev build-essential pkg-config patchelf
```

On macOS, install the Xcode Command Line Tools (`xcode-select --install`).

The live wallpaper engine in `engine/` is built automatically by `src-tauri/build.rs`.
To try live wallpapers on Linux, install `ffmpeg`. Windows and macOS bundle FFmpeg;
fetch it once before building there:

```bash
node scripts/fetch-ffmpeg.mjs
```

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

The bundled FFmpeg (Windows, macOS) is a trimmed LGPL build of `ffmpeg` and
`ffprobe` (FFmpeg n9.0.2, unmodified upstream source), made by
[.github/workflows/ffmpeg.yml](.github/workflows/ffmpeg.yml) and published as the
[`ffmpeg-n9.0.2-wf1`](../../releases/tag/ffmpeg-n9.0.2-wf1) release. Its license and
build notes (source commit and `configure` options) ship with the app.

## Branches and pull requests

Every change follows the same path. No pull request skips a step.

| Branch      | What it is                                                      |
| ----------- | --------------------------------------------------------------- |
| `main`      | Released code. Release tags `vX.Y.Z` are made here only.        |
| `dev`       | Integration. Everything lands here first.                       |
| `<feature>` | One branch per change, started from `dev` (e.g. `live-engine`). |

1. Start a feature branch from an up-to-date `dev`.
2. Open a pull request from the feature branch into **`dev`**. Never into `main`.
3. Merge it once CI is green on Linux, Windows and macOS.
4. To release, open a pull request from **`dev` into `main`**. This is the only
   pull request that targets `main`.
5. Tag `vX.Y.Z` on `main` to build the release. Bump the version before that with
   `yarn version:patch`, `version:minor` or `version:major`.
6. Delete the feature branch once it is merged.

## Versions

Wallflow uses [semantic versioning](https://semver.org): `MAJOR.MINOR.PATCH`.

- **Patch** (`1.1.1`, `1.1.2`, …): bug fixes and changes from issues and feedback.
- **Minor** (`1.2.0`, …): new features. Each feature release then gets its own patches.
- **Major** (`2.0.0`): big or breaking changes, such as a new config format or a
  dropped system.

Found a problem or have an idea? Open an [issue](../../issues).

## Notes and limitations

- WebP images are transcoded to PNG in the thumbnail cache before being applied,
  because Windows cannot render WebP wallpapers and GNOME only can with
  `webp-pixbuf-loader` installed.
- Closing the window hides it. Use _Quit_ in the tray menu to stop the app.
- Release builds send anonymous start and exit events to Aptabase. No account and no
  personal data. Debug builds and builds without `APTABASE_APP_KEY` send nothing.
- Per-monitor wallpapers are available on Windows and macOS; the Linux backends
  set every monitor to the same image.
- Live wallpapers need GNOME on Wayland on Linux (KDE and X11 not yet). Every
  monitor plays the same video.
- macOS has no wallpaper tile mode, so _Tile_ is applied as _Center_.
- Xfce only exposes its wallpaper properties after the Desktop Settings dialog has
  been opened once on a fresh install.

## License

MIT — see [LICENSE](LICENSE).
