# Wallflow — progress

Hand-off log between machines. Read this after `git pull`, before changing anything.

## Roadmap

Goal: replace the old per-platform live wallpaper (`live-wall` branch: GStreamer
inside gnome-shell, GDI on Windows) with one Rust engine that plays smoothly on
every platform without slowing the desktop.

| # | Step | Status |
| --- | --- | --- |
| 1 | Review and fix the non-live app (scheduler, config sync, file writes, UI bugs) | **done** — `review-fixes` (4193a7a) |
| 2 | Engine core: ffmpeg decode → wgpu NV12 render, frame pacing, stdin control | **done** — `engine/` |
| 3 | GNOME shim extension: desktop window above background, pause when hidden | **done**, verified headless + real session |
| 4 | Multi-monitor (one window per monitor, decoded once) | **done**, verified headless (2 monitors) |
| 5 | `prepare` import step (downscale, ≤30 fps, poster) | **done** |
| 6 | Integrate into the app: bundle engine as Tauri sidecar, Live page, prepare progress UI, start/stop through the shim | **next** |
| 7 | Shims for other desktops: Windows (WorkerW), macOS (desktop-level NSWindow), KDE/wlroots (wlr-layer-shell), X11 | planned |
| 8 | Zero-copy decode → texture (DMA-BUF / D3D11 / IOSurface) | planned |
| 9 | Merge `live-engine` → `dev` → `main`, release | planned |

## Branches

- `live-engine` — current work: `review-fixes` + `engine/` + the gallery preview
  feature (9d7fa5a, from another session). Pushed.
- `review-fixes` — app fixes only, pushed, not merged to `dev` yet.
- `live-wall` — the old live wallpaper. To be dropped; nothing from it is reused.

## Sessions

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

- Real session, visually checked by you: desktop playback, colours, fill, no
  lag, icons — OK. The overview fix is verified headless only; check it after
  the next login.
- Engine CPU % on a near-idle laptop reads high (~40% of one core) because cores
  idle at low clocks; with one other core busy the same run is ~15%. Energy
  (RAPL needs root here) is the right metric; not measured.
- Windows / macOS / KDE / X11: no shim yet, nothing tested.
- The shim reloads only at login on Wayland: after changing `engine/gnome-shim/`,
  log out and in (or use `engine/tests/gnome-headless/run.sh`).
- `fetch_latest_version` CSP still lists `http://127.0.0.1:5500` (dev leftover?).

## Next step

1. After logging out and in (loads the new shim), re-test dragging a window to
   another workspace in the overview, with the live wallpaper running. Then check the overview: minimize
   all windows, then

   ```bash
   gdbus call --session --dest com.efadhemon.Wallflow.LiveShim --object-path /com/efadhemon/Wallflow/LiveShim --method com.efadhemon.Wallflow.LiveShim.Start "['$PWD/engine/target/release/wallflow-live', '$HOME/Videos/14846689_1920_1080_30fps.mp4', '--background']"
   ```

   Hot corner → the overview should show the video frame (rounded corners), stay
   smooth, and the desktop should continue from that frame. `...LiveShim.Stop` ends it.
2. Step 6: app integration. Plan: build `wallflow-live` as a Tauri `externalBin`
   sidecar; Linux GNOME backend talks to the shim over D-Bus (`Start`/`Stop`/`Send`),
   offers to install/enable the shim when missing; Live page imports videos via
   `prepare` with a progress bar and uses the poster as the static wallpaper.

## Testing

```bash
cd engine && cargo test                                   # unit tests
engine/tests/gnome-headless/run.sh [VIDEO]                # end-to-end, throwaway shell
MONITORS="1920x1080 1280x720" engine/tests/gnome-headless/run.sh
EXTRA_EXTENSIONS="'ding@rastersoft.com'" engine/tests/gnome-headless/run.sh
SCRIPT=bench.sh engine/tests/gnome-headless/run.sh VIDEO  # CPU per state, old vs new
```
