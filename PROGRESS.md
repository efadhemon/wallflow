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

### 2026-11-05 — plan for steps 1–6; release build; W0 and FFmpeg started (host EMON)

- You chose to bundle FFmpeg. Full plan for steps 1–6 written (below, 1d479de).
- **1.1** engine release profile (f306fc4): sidecar 14.2 → 8.8 MB.
- **1.2** `yarn tauri build`: `.deb` 9.7 MB (1.0.5: 5.4 MB), contains
  `/usr/bin/wallflow-live`, `Depends: ffmpeg, …`; the app looks for the engine
  beside itself, so `/usr/bin` works. AppImage 89 MB. Not installed yet.
- **3.1** `prepare` picks libx264 → h264_videotoolbox → h264_mf (980dcc6);
  real prepare with libx264 unchanged.
- **3.2** `--events` (82c6419): `ready`, `state` (only on change), `error`;
  checked with a real run (pause twice → one event; missing file → error).
- **2.1–2.3** (5f0fa31): FFmpeg build script, workflow, fetch script. Local
  Linux build of the same flags: ffmpeg + ffprobe ≈ 5.2 MB compressed, LGPL,
  only libc / libm; the engine plays a video with it; prepare reports "no H.264
  encoder" cleanly (expected, Linux build has none). Pinned by commit, not
  tarball hash: ffmpeg.org reset the connection here, and GitHub archive hashes
  are not guaranteed stable.
- **2.5** (e56a117): bundled FFmpeg first, then PATH; passed to the engine
  for playback and prepare. Headless GNOME `app.sh`: all 6 checks pass.
- Git over SSH failed for a while (`Permission denied (publickey)`): a GitHub
  issue, works again. Pushed over HTTPS meanwhile.
- You installed the `.deb`: `/usr/bin/wallflow` runs from autostart and plays
  the live wallpaper through `/usr/bin/wallflow-live`. (That package was built
  before e56a117, so it still runs FFmpeg from PATH.) apt's note "couldn't be
  accessed by user '_apt'" is harmless: `/home/efadh` is `drwxr-x---`, so apt
  read the local `.deb` as root instead of its sandbox user.
- CI run 41975083253 (b24c87d): green on Linux, macOS and Windows.

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

- FFmpeg on Windows / macOS: **decided, bundled** (Plan, step 2). Earlier size
  estimates: `setup.exe` ≈ 10–15 MB, `.dmg` ≈ 13–18 MB; measure in 2.7.
- Real session, visually checked by you: desktop playback, colours, fill, no
  lag, icons, overview, moving windows between workspaces — OK.
- Release CI builds the engine for every target via `build.rs`; it has only been
  compiled on Linux. The macOS / Windows release jobs may need fixes.
- Autostart on host EMON: `/usr/bin/wallflow` from the new `.deb` (2026-11-05).
- **Size (Linux):** 1.0.5 `.deb` was 5.4 MB. The engine sidecar adds 14.2 MB
  as built now (≈4.5 MB compressed): `engine/Cargo.toml` has no release
  profile. With strip + LTO it is 6.3 MB (≈2.1–2.7 MB compressed). FFmpeg
  itself is not in the package (apt dependency, already installed on Ubuntu
  desktops that play video).
- **AppImage has no FFmpeg:** `depends: ffmpeg` only applies to the `.deb`; an
  AppImage user without system FFmpeg sees the "FFmpeg missing" banner.
- Engine CPU % on a near-idle laptop reads high (~40% of one core) because cores
  idle at low clocks; with one other core busy the same run is ~15%. Energy
  (RAPL needs root here) is the right metric; not measured.
- Windows / macOS / KDE / X11: no shim yet, nothing tested.
- The shim reloads only at login on Wayland: after changing `engine/gnome-shim/`,
  log out and in (or use `engine/tests/gnome-headless/run.sh`).
- `fetch_latest_version` CSP still lists `http://127.0.0.1:5500` (dev leftover?).

## Next step

1. **You approve, then run** the FFmpeg workflow (creates a pre-release in the
   repo): `gh workflow run ffmpeg.yml --ref live-engine`. Then copy the
   release's `SHA256SUMS` into `scripts/ffmpeg/SHA256SUMS` and do 2.4
   (externalBin + fetch step in CI / release) — needs the release first, or the
   Windows / macOS CI jobs fail.
2. Meanwhile: 3.3 (engine `platform` module) and 3.4 (`direct` backend).

## Plan (steps 1–6)

Decided 2026-11-05: **FFmpeg n9.0.2 is bundled** on Windows / macOS (option 1), as a
trimmed **static LGPL** build of `ffmpeg` + `ffprobe`. Static, not shared
libraries: two `externalBin` sidecars, nothing to place or sign separately;
both binaries are entirely LGPL code, so static linking is fine for the license.

### Step 1 — Linux release (done: f306fc4, `.deb` installed and playing)
- 1.1 Engine release profile (strip, LTO, 1 codegen unit; opt-level 3 because
  the render loop is hot; panic stays unwind so a decoder-thread panic does not
  kill playback). Measure the size.
- 1.2 `yarn tauri build` → `.deb` + AppImage. Check with `dpkg -c` / `dpkg -I`:
  `/usr/bin/wallflow-live` present, `Depends: ffmpeg`. You install it and check
  autostart + live wallpaper after login.

### Step 2 — FFmpeg bundle (2.1–2.3 and 2.5 done 5f0fa31, e56a117; 2.4, 2.6, 2.7 left)
- 2.1 `scripts/ffmpeg/build.sh <target>`: pinned FFmpeg 8.0.x source + sha256,
  `--disable-everything` then only what the engine uses:
  protocols `file,pipe`; demuxers `mov,matroska,avi,mpegts,image2`; muxers
  `mp4,rawvideo,image2`; decoders `h264,hevc,vp8,vp9,av1,mpeg4,mjpeg,png`;
  parsers for those; encoders `mjpeg,rawvideo` + `h264_mf` (Windows) /
  `h264_videotoolbox` (macOS); filters `buffer,buffersink,null,format,scale,fps,setsar`;
  hwaccels `*_d3d11va` / `*_videotoolbox`; `ffmpeg` + `ffprobe`, no ffplay,
  no network, no docs. No `--enable-gpl`, no `--enable-nonfree`.
- 2.2 Workflow `ffmpeg.yml` (manual): Windows x86_64 (mingw-w64 cross build
  on Ubuntu), macOS arm64 + x86_64 (macos-14). Publishes a GitHub release
  `ffmpeg-<version>-wf<n>` with one archive per target, `SHA256SUMS`, the
  source tarball and the configure line (LGPL source duty).
- 2.3 `scripts/fetch-ffmpeg.mjs <triple>`: downloads the pinned archive,
  checks it against `scripts/ffmpeg/SHA256SUMS` (checked in), writes
  `src-tauri/binaries/{ffmpeg,ffprobe}-<triple>[.exe]`. CI and release run it
  before building; `build.rs` stops with a clear message if they are missing.
- 2.4 `tauri.windows.conf.json` / `tauri.macos.conf.json` add both to
  `externalBin` (Linux keeps the apt dependency).
- 2.5 App: `ffmpeg_paths()`: bundled next to the executable first, then
  `PATH`. Passed to the engine (`--ffmpeg` / `--ffprobe`) for playback and
  `prepare`; `ffmpeg_available()` checks the same paths.
- 2.6 License: `licenses/FFmpeg-LGPL-2.1.txt` bundled, a line in Settings →
  About with the version and a link to the source release.
- 2.7 Measure the real `setup.exe` / `.dmg` sizes.

### Step 3 — W0, common groundwork (3.1 done 980dcc6, 3.2 done 82c6419)
- 3.1 `prepare` picks the encoder from `ffmpeg -encoders`: `libx264` (CRF 20)
  → `h264_mf` / `h264_videotoolbox` (bitrate ≈ 0.1 bit per pixel per frame,
  e.g. 1080p30 ≈ 6 Mbit/s) → error. Pure function, unit tested with recorded
  encoder lists.
- 3.2 Engine prints JSON lines on stdout while playing, with `--events`: `ready` (screens),
  `state` (`playing` / `paused`, why), `error`. The GNOME shim ignores them.
- 3.3 Engine `platform` module: `attach_background(window, monitor)` and a
  cover watcher that sends pause / resume into the event loop. Linux: no-op
  (the shim does both). Pause per window: a covered monitor stops drawing;
  decoding stops only when every monitor is covered.
- 3.4 App `live/`: backends `gnome` (as now) and `direct` (the app starts the
  engine itself, keeps its stdin, reads its events, the supervisor restarts it).
  `WALLFLOW_LIVE_BACKEND=direct` runs `direct` on Linux for testing (plain
  windows, not behind the desktop).

### Step 4 — W1, Windows
- 4.1 Attach: send `0x052C` to `Progman`; find the `WorkerW` behind the icons
  (Windows 10 / 11 ≤ 23H2: sibling `WorkerW` after `SHELLDLL_DefView`; 11 24H2+:
  child of `Progman`); `SetParent` each monitor window; positions relative to
  the virtual screen (may start at negative x/y); per-monitor DPI v2.
- 4.2 Cover: `SetWinEventHook` (foreground, move/size, minimize/restore) →
  a maximized or fullscreen window on that monitor; `SHQueryUserNotificationState`
  (fullscreen games, presentations); `WTSRegisterSessionNotification` (lock,
  user switch); display off / sleep via `RegisterPowerSettingNotification`.
- 4.3 Explorer restart: `TaskbarCreated` → find `WorkerW` again, re-attach.
- 4.4 Decode `-hwaccel d3d11va`; `prepare` uses `h264_mf`.
- 4.5 App: `direct` backend; Support `Ready` when engine + FFmpeg present;
  poster as the static wallpaper (`wallpaper/windows.rs`).
- 4.6 Test on a real PC: Windows 10 and 11 24H2, 2 monitors with different
  DPI, icons on top and clickable, maximize / fullscreen game / lock / sleep,
  Explorer restart, CPU idle when covered.

### Step 5 — W2, macOS
- 5.1 Attach: one borderless `NSWindow` per `NSScreen` at
  `kCGDesktopWindowLevel`, `canJoinAllSpaces | stationary | ignoresCycle`,
  `ignoresMouseEvents`; engine activation policy `accessory` (no Dock icon).
- 5.2 Cover: `NSWindow` occlusion-state notifications; `NSWorkspace` screens
  sleep / wake, session resign / become active; screen lock
  (`com.apple.screenIsLocked` / `…Unlocked`).
- 5.3 Decode `-hwaccel videotoolbox`; `prepare` uses `h264_videotoolbox`.
- 5.4 Signing: engine, ffmpeg, ffprobe signed with hardened runtime by the
  Tauri bundler, notarized with the app; per-arch builds as in `release.yml`.
- 5.5 App: `direct` backend; poster on every Space (`wallpaper/macos.rs`).
- 5.6 Test on an Apple Silicon Mac: Spaces, fullscreen app, Mission Control,
  external monitor, sleep / lock, Stage Manager.

### Step 6 — the rest
- 6.1 KDE Plasma / wlroots: `wlr-layer-shell` background layer (winit cannot,
  needs smithay-client-toolkit for that path).
- 6.2 X11: `_NET_WM_WINDOW_TYPE_DESKTOP` window below the icons.
- 6.3 Native playback / zero-copy (old step 8).
- 6.4 Merge `live-engine` → `dev` → `main`, release 1.1.0, drop `live-wall`.

Needs from you: a Windows PC for 4.6 and a Mac for 5.6.

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
