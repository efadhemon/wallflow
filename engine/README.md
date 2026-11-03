# wallflow-live

Wallflow's live (video) wallpaper engine. It replaces the per-platform players
from the `live-wall` branch with one renderer that behaves the same everywhere.

## Why it is built this way

The old implementation decoded video inside the desktop shell on GNOME (a
GStreamer pipeline in the shell extension) and drew frames on the CPU through GDI
on Windows. Both put work on threads the desktop needs for its own animations,
which is why the overview, the hot corner and window switching stuttered.

The engine follows three rules:

1. **Its own process.** `wallflow-live` is a separate binary. The desktop shell
   only composites its window, so a slow frame can never stall the shell, and a
   crash can never take the app or the desktop with it.
2. **GPU from decode to screen.** FFmpeg decodes with the hardware decoder and
   hands over NV12 frames. They are uploaded as two textures, and one wgpu draw
   (Vulkan / Metal / Direct3D 12) does the YUV→RGB conversion and the scaling.
3. **No work when nobody can see it.** Only a frame that was actually drawn lets
   the next one be decoded. When the desktop is covered (overview, a maximized or
   fullscreen window, the lock screen) the engine is paused, FFmpeg blocks on a
   full pipe, and CPU use drops to zero.

```
 Wallflow app ──stdin JSON──▶ wallflow-live ──▶ wgpu window ──▶ desktop shim
                               ▲                                 (GNOME extension /
                    ffmpeg ────┘ NV12 frames over a pipe          WorkerW / layer-shell)
                    (hw decode)
```

## Layout

| Path | What |
| --- | --- |
| `src/probe.rs` | `ffprobe` → size, frame rate, colour matrix and range |
| `src/decoder.rs` | `ffmpeg` child process, bounded frame queue, buffer recycling |
| `src/fit.rs` | fill / fit / stretch placement (pure, unit tested) |
| `src/render.rs` | wgpu NV12 → RGB renderer |
| `src/main.rs` | event loop, frame pacing, stdin control, decoder restarts |
| `gnome-shim/` | GNOME Shell extension that places the window behind the desktop |
| `tests/gnome-headless/` | end-to-end test in a throwaway headless gnome-shell |

## Running

```bash
cargo build --release
./target/release/wallflow-live ~/Videos/clip.mp4                 # in a normal window
./target/release/wallflow-live ~/Videos/clip.mp4 --fill fit --hwaccel none
```

Options: `--fill fill|fit|stretch`, `--hwaccel auto|none|vaapi|cuda|…` (default
`auto`; a decoder that yields no frames falls back to software), `--background`
(borderless, monitor-sized, click-through; used by the desktop shims),
`--ffmpeg PATH`, `--ffprobe PATH`.

Preparing a video when it is added (once):

```bash
./target/release/wallflow-live prepare IN.mp4 --out OUT.mp4 --poster OUT.jpg --width 1920 --height 1080
```

It downscales (never upscales) so the video still covers the screen, caps it at
30 fps, re-encodes to H.264 4:2:0 without audio on at most half the cores, and
saves a poster frame (for the static wallpaper and the library tile). Progress
is printed as JSON lines: `{"event":"started",…}`, `{"event":"progress","fraction":0.42}`,
`{"event":"done",…}`. A 2560×1440 clip prepared for a 1080p screen plays with
**37% less CPU** (51% → 32.5% of one core, measured back to back under the same load).

Control, one JSON object per line on stdin; the engine exits when stdin closes:

```json
{"cmd":"pause"}  {"cmd":"resume"}  {"cmd":"fill","mode":"fit"}  {"cmd":"quit"}
```

## GNOME

GNOME has no protocol for background windows, so `gnome-shim/` does the minimum
inside the shell: it starts the engine as a trusted Wayland client
(`Meta.WaylandClient`), makes its window a sticky `DESKTOP` window hidden from the
dock, Alt-Tab and the overview, keeps it directly above the background (desktop
icons stay on top), and sends `pause` / `resume` as the desktop is covered and
uncovered. It never touches a video frame.

D-Bus API on the session bus, `com.efadhemon.Wallflow.LiveShim` at
`/com/efadhemon/Wallflow/LiveShim`: `Start(as argv)`, `Stop()`, `Send(s line)`,
properties `Version`, `Running`, `Paused`. `Start` only accepts a path to a
`wallflow-live` executable.

To try it in your own session: `engine/gnome-shim/install.sh` (turns off the old
in-shell player; `--remove` undoes it), log out and back in, then start the
engine through the shim's `Start` method as the script prints.

In background mode the engine opens one window per monitor, titled with that
monitor's origin (`Wallflow Live @x,y`), and the shim puts each on its monitor.
One decode and one upload per frame serve every monitor. Playback pauses only
when no monitor shows any desktop.

```bash
tests/gnome-headless/run.sh [VIDEO]                          # needs gnome-shell, gnome-text-editor
MONITORS="1920x1080 1280x720" tests/gnome-headless/run.sh    # two monitors
```

## Measured (Ubuntu, GNOME 50 Wayland, i7-8750H, UHD 630, 1080p30 H.264)

| Decoder | engine | ffmpeg | total (one core = 100%) |
| --- | --- | --- | --- |
| software | 7.0% | 50.5% | 57% |
| `auto` (NVDEC here) | 6.2% | 9.6% | **16%** (≈1.3% of the machine) |
| paused / covered | 0% | 0% | 0% |

30.1 fps drawn, 0 dropped. The engine renders on the integrated GPU
(`PowerPreference::LowPower`).

## Next

- Per-monitor videos (all monitors currently show the same video, decoded once).
- Shims for Windows (WorkerW), macOS (desktop-level `NSWindow`), wlroots/KDE
  (`wlr-layer-shell`) and X11 (`_NET_WM_WINDOW_TYPE_DESKTOP`).
- Zero-copy decode → texture (DMA-BUF / D3D11 shared handles / IOSurface).
- Wire it into the Wallflow app and bundle it as a Tauri sidecar.
