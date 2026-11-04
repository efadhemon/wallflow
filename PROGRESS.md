# Wallflow — progress

Hand-off log between machines. Read this after `git pull`, before changing anything.

## Roadmap

Goal: replace the old per-platform live wallpaper (`live-wall` branch: GStreamer
inside gnome-shell, GDI on Windows) with one Rust engine that plays smoothly on
every platform without slowing the desktop.

| #   | Step                                                                                                                | Status                                            |
| --- | ------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------- |
| 1   | Review and fix the non-live app (scheduler, config sync, file writes, UI bugs)                                      | **done** — `review-fixes` (4193a7a)               |
| 2   | Engine core: ffmpeg decode → wgpu NV12 render, frame pacing, stdin control                                          | **done** — `engine/`                              |
| 3   | GNOME shim extension: desktop window above background, pause when hidden                                            | **done**, verified headless + real session        |
| 4   | Multi-monitor (one window per monitor, decoded once)                                                                | **done**, verified headless (2 monitors)          |
| 5   | `prepare` import step (downscale, ≤30 fps, poster)                                                                  | **done**                                          |
| 6   | Integrate into the app: bundle engine as Tauri sidecar, Live page, prepare progress UI, start/stop through the shim | **done** (GNOME), checked by you in the real app |
| 7   | Shims for other desktops: Windows (WorkerW), macOS (desktop-level NSWindow), KDE/wlroots (wlr-layer-shell), X11     | planned                                           |
| 8   | Zero-copy decode → texture (DMA-BUF / D3D11 / IOSurface)                                                            | planned                                           |
| 9   | Merge `live-engine` → `dev` → `main`, release                                                                       | planned                                           |

## Branches

- `live-engine` — current work: `review-fixes` + `engine/` + the gallery preview
  feature (9d7fa5a, from another session). Pushed.
- `review-fixes` — app fixes only, pushed, not merged to `dev` yet.
- `live-wall` — the old live wallpaper. To be dropped; nothing from it is reused.

## Sessions

### 2026-11-04 — real-app test passed on host EMON (live-engine)

- You ran `yarn tauri dev` for a long while and checked the Live page in the
  real window: everything worked. Shim `wallflow-live@com.efadhemon.wallflow`
  is now installed on this machine.
- CI on `live-engine` (run 41790324676) failed on macOS and Windows: an
  unused `mut` in the engine outside Linux, and `build.rs` passed clippy's
  wrapper env into the nested engine build. Fixed in cf8d291, plus GNOME-only items allowed as dead code elsewhere and explicit
  engine fmt / clippy / test steps in CI. Cross-clippy from Linux is not
  possible (ring / objc2 need the target's C compiler), so CI is the check.
- Windows / macOS plan written (section below).

### 2026-11-04 — status check on host EMON (live-engine)

- Pulled `live-engine` (head 01d1c4f); this log matched the git history.
- On this machine: `engine` 10 and `src-tauri` 31 unit tests pass, sidecar
  builds into `src-tauri/binaries/`. The shim is not installed here yet, the old
  in-shell extension is not either. No code changes; Next step rewritten as a
  concrete test checklist for this machine.

### 2026-11-04 — engine integrated into the app (live-engine)

- **Build:** `src-tauri/build.rs` builds `engine/` for the target triple and
  copies it to `src-tauri/binaries/wallflow-live-<triple>` (Tauri
  `externalBin`). Every Rust build (dev, build, check, clippy, CI) gets it; no
  yarn step. `.deb` now depends on `ffmpeg`. `binaries/` and `engine/target` are
  git-ignored; eslint/prettier skip `engine/` (GNOME/GJS code style).
- **Backend `src-tauri/src/live/`:** library in `<app data>/live/<id>/`
  (`video.mp4`, `poster.jpg`, `meta.json`; id = hash of size + first/last 4 MiB,
  so re-adding a file is free); import runs `wallflow-live prepare` for the
  largest monitor with `live://import` progress events; `gnome.rs` drives the
  shim over D-Bus (`gdbus`) and can install + enable it (copy compiled in,
  old in-shell extension turned off); `supervise` keeps the selected video
  playing (login, shell restart, engine crash), 2 s at first then every 15 s.
- **Config:** `liveVideo` (backend-owned; `set_config` keeps the backend's
  value). While set, the timer rotation is suspended and the poster is the
  static wallpaper; Next / Previous / picking an image ends the live wallpaper;
  a fill-mode change goes to the engine instead. Quitting the app stops it.
- **Commands:** `live_status`, `live_list`, `live_import`, `live_remove`,
  `live_play`, `live_stop`, `live_install_helper`.
- **UI:** new **Live** page (sidebar): setup banners (GNOME helper / log in
  again / unsupported / FFmpeg missing), now-playing card (Playing / Paused while
  the desktop is covered / Starting…, Stop), poster grid with Play and remove,
  import tiles with progress. Dashboard banner and sidebar status "Live
  wallpaper" while playing.
- **Bug found and fixed:** locking the screen disabled the shim (GNOME turns
  extensions off on lock), which ended the live wallpaper for good. The shim now
  declares `"session-modes": ["user", "unlock-dialog"]` and only pauses there;
  the app's supervisor covers anything else that kills the engine.
- **Dev tool:** `yarn dev` opened in a plain browser runs the UI against a
  Tauri stand-in with sample data (`src/dev/browserMock.ts`, dev builds only;
  `window.__mock` switches states). `.claude/launch.json` starts vite.
- **Verified:** 31 Rust unit tests; e2e import with the real sidecar + FFmpeg
  (`WALLFLOW_TEST_VIDEO=… cargo test -- --ignored`); headless GNOME suites (1 and
  2 monitors, DING, lock/unlock) and a new `SCRIPT=app.sh` run of the real app:
  restores the live wallpaper at startup, rotation leaves it alone, survives
  lock/unlock, restarts a killed engine. UI states checked in the browser with
  the stand-in. **Not yet:** clicking through the real app window in your
  session, and a release build (`yarn tauri build`) / Windows / macOS CI.

### 2026-11-04 — overview: windows could not be moved to another workspace (live-engine)

- Your screenshots (video off vs on) showed it: with the video running the
  current workspace card stretched to full width and the next workspace no
  longer peeked in at the right edge, so there was nowhere to drop a window.
- Cause: GNOME sizes the workspace card from its children's preferred size; the
  video `Clutter.Clone` asked for its source's size (the whole monitor). Fix:
  `CoverClone` requests no size and only fills the box it is given.
- New checks (both fail on the old shim, pass now): the overview card has the
  same size with and without the video (old: 1866 vs 1510 px), and the right
  edge is the next workspace's drop target (old: workspace 0, now 1).

### 2026-11-04 — overview: video covered after a wallpaper change (live-engine)

- Your report + screen recording: in the overview a window could not be moved
  to another workspace, and mid-drag the workspace card went black, then showed
  the old static image (sunset).
- Root cause of the black/sunset: the Wallflow app (old debug build from
  autostart) rotated the static wallpaper while the overview was open. GNOME's
  `BackgroundManager` then `add_child`s a new background actor on **top** of the
  workspace background group — black until it loads, then the new image —
  covering the video clone. Fixed: the shim watches `child-added`, keeps new
  background actors hidden and the clone on top. Regression check added (fails
  without the fix, passes with it).
- The drag failure itself was found in the next session (card layout).
- When integrating: the app must stop rotating static wallpapers while a live
  wallpaper plays (and set the poster as the static one).

### 2026-11-04 — overview shows the video (live-engine)

- Your report after the real-session test: colours and fill right, no lag,
  icons on top and clickable — but the **overview showed the old static image**
  and the video only came back after leaving it.
- Fix in the shim: on overview `showing`, a clone of the video window is laid
  over every workspace background (per monitor). Playback is paused there, so the
  overview shows the frame it stopped on and the desktop resumes from it.
  - `CoverClone`: scaled evenly to cover the workspace box (its aspect differs
    from the monitor's — a plain clone squashed the video ~19%).
  - `RoundedLikeBackground` (GLSL): same rounded corners as GNOME's card, read
    from the background it covers; corners only, so no strip along the edges.
    A Clone paints in its source's size, which the shader has to account for.
  - The static background under the clone is hidden while it is there.
- Headless test: new check "overview shows the video" + overview screenshot;
  `DEBUG_GEOMETRY=1` prints the overview layout. All checks pass with 1 and 2
  monitors and with DING.
- **Needs a logout/login** to load the new shim code in the real session.

### 2026-11-04 — engine tested in the real session (live-engine)

- Installed the shim with `engine/gnome-shim/install.sh` (old
  `live-wallpaper@com.efadhemon.wallflow` turned off), logged out and in: shim
  is ACTIVE, D-Bus works.
- Real session: started through the shim, the engine came up and paused at once,
  because a maximized window covered the desktop — correct. Forced `resume`:
  plays at 30 fps; gnome-shell 23% → 31% while compositing it.
- Mutter keeps sending frame callbacks to a covered window, so the shim's pause
  (overview / maximized / fullscreen / locked) is what keeps a hidden wallpaper
  free, not occlusion.
- Headless test now also runs with DING (`EXTRA_EXTENSIONS="'ding@rastersoft.com'"`):
  order is background → video → desktop icons → apps; all 12 checks pass.
- Old vs new in the same headless shell, 1080p30: old player kept gnome-shell
  ~90% busy (94% in the overview, still playing); new: shell ~9%, 1.4% in the
  overview (paused). This was the hot-corner / overview lag.

### 2026-11-04 — engine built (e9eba24 … ca60256)

- `engine/` crate `wallflow-live`, GNOME shim, headless test + benchmark,
  `prepare`, multi-monitor, decoder restart/fallback, 1 MiB decoder pipe.
- vite no longer watches `engine/` (was reloading the page on every cargo build).
- Details and numbers: `engine/README.md`.

### 2026-11-04 — review of the non-live app (4193a7a, `review-fixes`)

- Fixed: exclusions reverted by stale config, macOS per-monitor deadlock, missed
  watcher events, disk scan under the state lock, `expectedSize` arg name,
  number input "15"→"105", over-eager reloads, non-atomic writes, hanging
  desktop tools, EXIF thumbnails.

## Open / not verified

- **Open decision — FFmpeg on Windows / macOS** (Linux: `.deb` dependency).
  `prepare` uses `libx264` (GPL); an LGPL FFmpeg has none, so every LGPL
  option also needs the hardware-encoder switch in W0.
  1. Bundle LGPL `ffmpeg` + `ffprobe` as sidecars. Works offline, one tested
     version. Stock builds are large; a trimmed custom build (our decoders,
     demuxers, scale, mjpeg, h264_mf / videotoolbox) is much smaller but needs
     its own CI build. LGPL notice + source link required.
  2. Download on first use of the Live page. Small installer; needs internet,
     hosting, checksum check, an error / retry UI, and the binaries still have to
     be signed (macOS arm64) and may trip antivirus. Same license duty.
  3. Link libav* into the engine. No pipe, the way to zero-copy, but FFmpeg dev
     libraries per target in CI, LGPL means dynamic linking (dlls / dylibs ship
     anyway), unsafe FFI, `prepare` rewritten or still needs the CLI.
  Recommendation: 1 with a trimmed build now; native playback later (see plan).

- Real session, visually checked by you: desktop playback, colours, fill, no
  lag, icons, overview, moving windows between workspaces — OK.
- Release CI builds the engine for every target via `build.rs`; it has only been
  compiled on Linux. The macOS / Windows release jobs may need fixes.
- Autostart on host EMON runs the installed 1.0.5 (`/usr/bin/wallflow`), which
  has no live engine: replace it with the new `.deb` (Next step 1).
- Engine CPU % on a near-idle laptop reads high (~40% of one core) because cores
  idle at low clocks; with one other core busy the same run is ~15%. Energy
  (RAPL needs root here) is the right metric; not measured.
- Windows / macOS / KDE / X11: no shim yet, nothing tested.
- The shim reloads only at login on Wayland: after changing `engine/gnome-shim/`,
  log out and in (or use `engine/tests/gnome-headless/run.sh`).
- `fetch_latest_version` CSP still lists `http://127.0.0.1:5500` (dev leftover?).

## Next step

1. Check the CI run on `live-engine` (run 41797820483, after cf8d291): all
   three OSes must be green before any Windows / macOS work.
2. **You decide FFmpeg for Windows / macOS** (options under Open decisions).
3. Linux release build: `yarn tauri build`, install the `.deb` over 1.0.5
   (sidecar bundled and found, `ffmpeg` dependency, autostart restores the live
   wallpaper).
4. Then the Windows / macOS plan below, phase W0 first.

## Windows / macOS plan

**W0 — common groundwork (Linux-testable)**
- Engine reports its state on stdout as JSON lines (`{"event":"state","paused":…}`,
  `{"event":"error",…}`), because on Windows / macOS no shim sits in between:
  the app starts the engine itself and keeps its stdin.
- App: split `live/` into backends behind one interface (`gnome.rs`,
  `windows.rs`, `macos.rs`; start / stop / send / state). The supervisor
  restarts a child process that exited.
- `prepare` picks the encoder from `ffmpeg -encoders`: `libx264` if present
  (Linux distro FFmpeg), else `h264_mf` (Windows) / `h264_videotoolbox` (macOS),
  bitrate-based (`-b:v` from size × fps) because hardware encoders ignore CRF.
- FFmpeg delivery as decided (bundle / download / link).

**W1 — Windows**
- Engine `--background`: send `0x052C` to `Progman`, find the `WorkerW` behind
  the icons (Windows 11 24H2 moved it under `Progman`: handle both layouts),
  `SetParent` each monitor window into it, positions relative to the virtual
  screen (it can start at negative x/y), per-monitor DPI v2.
- Pause per monitor: `SetWinEventHook` (foreground / move-size / minimize) →
  a maximized or fullscreen window covers that monitor;
  `SHQueryUserNotificationState` for fullscreen games / presentations;
  `WTSRegisterSessionNotification` for lock and user switch.
- Explorer restart: `TaskbarCreated` message → find `WorkerW` again, re-parent.
- Decode: `-hwaccel d3d11va`. Poster as the static wallpaper (existing
  `wallpaper/windows.rs`).
- Test: Windows 10 and 11 (24H2) on a real machine, 2 monitors, mixed DPI.

**W2 — macOS**
- Engine `--background`: one `NSWindow` per `NSScreen` at
  `kCGDesktopWindowLevel`, `canJoinAllSpaces | stationary | ignoresCycle`,
  `ignoresMouseEvents`; activation policy `accessory` so the engine gets no
  Dock icon.
- Pause: `NSWindow` occlusion state (macOS reports it, unlike Mutter),
  `NSWorkspace` screens sleep / wake and session resign, screen lock
  (`com.apple.screenIsLocked`).
- Decode: `-hwaccel videotoolbox`. Poster on every Space via the existing
  `wallpaper/macos.rs`.
- Release: engine + FFmpeg signed with hardened runtime and notarized with
  the app (Tauri signs `externalBin`); per-arch (arm64 / x86_64) like the
  current release matrix.
- Test: a real Mac (Apple Silicon), Spaces, a fullscreen app, external monitor.

**Later (step 8):** playback only ever sees `prepare`d H.264 MP4, which the OS
decodes natively. Native players (Media Foundation → D3D11 texture,
AVFoundation → IOSurface → Metal) would give zero-copy and remove FFmpeg from
playback; FFmpeg would then be needed only for `prepare`.

## Testing

```bash
cd engine && cargo test                                   # unit tests
engine/tests/gnome-headless/run.sh [VIDEO]                # end-to-end, throwaway shell
MONITORS="1920x1080 1280x720" engine/tests/gnome-headless/run.sh
EXTRA_EXTENSIONS="'ding@rastersoft.com'" engine/tests/gnome-headless/run.sh
SCRIPT=bench.sh engine/tests/gnome-headless/run.sh VIDEO  # CPU per state, old vs new
SCRIPT=app.sh engine/tests/gnome-headless/run.sh          # the real app (cargo build first)
SCRIPT=scenarios.sh engine/tests/gnome-headless/run.sh    # overview drag / wallpaper-change diagnostics
cd src-tauri && cargo test                                # app unit tests
WALLFLOW_TEST_VIDEO=~/Videos/clip.mp4 cargo test -- --ignored --test-threads 1   # real import (+ shim)
yarn dev   # then open http://localhost:1420 in a browser: UI with sample data
```
