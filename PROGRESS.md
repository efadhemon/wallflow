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

- **Visual check in the real session not done yet**: every window was maximized,
  so the shim correctly kept it paused. Needs one look with the desktop visible
  (minimize windows, then start it — commands below).
- Engine CPU % on a near-idle laptop reads high (~40% of one core) because cores
  idle at low clocks; with one other core busy the same run is ~15%. Energy
  (RAPL needs root here) is the right metric; not measured.
- Windows / macOS / KDE / X11: no shim yet, nothing tested.
- The shim reloads only at login on Wayland: after changing `engine/gnome-shim/`,
  log out and in (or use `engine/tests/gnome-headless/run.sh`).
- `fetch_latest_version` CSP still lists `http://127.0.0.1:5500` (dev leftover?).

## Next step

1. Visual check (30 s): minimize all windows, then

   ```bash
   gdbus call --session --dest com.efadhemon.Wallflow.LiveShim --object-path /com/efadhemon/Wallflow/LiveShim --method com.efadhemon.Wallflow.LiveShim.Start "['$PWD/engine/target/release/wallflow-live', '$HOME/Videos/14846689_1920_1080_30fps.mp4', '--background']"
   ```

   Hot corner / overview should stay smooth; `...LiveShim.Stop` ends it.
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
