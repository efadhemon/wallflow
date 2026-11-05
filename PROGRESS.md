# Wallflow — progress

Hand-off log between machines. Read this after `git pull`, before changing anything.

## Summary — start here (2026-11-05)

**Where things are**

- Repository is **public**. Website: **https://efadhemon.github.io/wallflow/**
  (GitHub Pages, `pages.yml`; no custom domain any more). Downloads and the
  in-app update check come from GitHub releases.
- `main` = 9cef6fe, `dev` = 57e962c: PR #9 (`live-engine` → `dev`) and PR #10
  (`dev` → `main`) merged. Version is **1.1.0** in the code, but **1.1.0 is
  not released yet** (no tag). Latest release is still 1.0.6.
- `deps-update` (from `dev`, pushed, no PR yet): Tauri update, `cn` package,
  clean-up. Waiting for you to check the app locally (`yarn tauri dev`).
- Live wallpapers work on GNOME (Ubuntu); Windows / macOS build, Live page
  says "coming soon".

**Next session, in order**

1. **Check `deps-update` locally** (`yarn tauri dev`: wallpapers, gallery,
   library, live page, tray, autostart, dialogs, update banner). If good: PR
   `deps-update` → `dev`, merge, then `dev` → `main`.
2. **Release 1.1.0 (live wallpaper on Linux/GNOME):** rehearsal release
   (draft) from `main`; test `.deb` (helper setup, logout, play) + AppImage
   (FFmpeg banner) on Ubuntu and that Windows / macOS install and say "coming
   soon"; measure installer sizes (Plan 2.7); then tag `v1.1.0`. This machine
   runs the 1.0.6 `.deb`: install the 1.1.0 draft to get live wallpapers back.
3. **1.2.0 Windows** (Plan step 4, with 3.3), needs a Windows PC.
4. **1.3.0 macOS** (Plan step 5), needs a Mac. Later: KDE / X11 / zero-copy.

**Branch flow:** feature branch → PR into `dev` → PR `dev` → `main` → tag
`vX.Y.Z` on `main` releases. Keep only `main`, `dev` and active feature
branches. The old `live-wall` player is kept as tag `archive/live-wall`.

## Roadmap

Goal: replace the old per-platform live wallpaper (`live-wall` branch: GStreamer
inside gnome-shell, GDI on Windows) with one Rust engine that plays smoothly on
every platform without slowing the desktop.

| #   | Step                                                                                                                | Status                                           |
| --- | ------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------ |
| 1   | Review and fix the non-live app (scheduler, config sync, file writes, UI bugs)                                      | **done**, in `main` (1.0.6)                      |
| 2   | Engine core: ffmpeg decode → wgpu NV12 render, frame pacing, stdin control                                          | **done** — `engine/`                             |
| 3   | GNOME shim extension: desktop window above background, pause when hidden                                            | **done**, verified headless + real session       |
| 4   | Multi-monitor (one window per monitor, decoded once)                                                                | **done**, verified headless (2 monitors)         |
| 5   | `prepare` import step (downscale, ≤30 fps, poster)                                                                  | **done**                                         |
| 6   | Integrate into the app: bundle engine as Tauri sidecar, Live page, prepare progress UI, start/stop through the shim | **done** (GNOME), checked by you in the real app |
| 7   | Shims for other desktops: Windows (WorkerW), macOS (desktop-level NSWindow), KDE/wlroots (wlr-layer-shell), X11     | planned                                          |
| 8   | Zero-copy decode → texture (DMA-BUF / D3D11 / IOSurface)                                                            | planned                                          |
| 9   | Merge `live-engine` → `dev` → `main`, release                                                                       | merged; **release 1.1.0 next**                   |

## Branches

- `main` — released code; tags `vX.Y.Z` release from here.
- `dev` — integration; feature branches merge here first, then `dev` → `main`.
- `live-engine` — the live wallpaper engine, merged into `dev` / `main`
  (PR #9, #10). Can be deleted.
- `deps-update` — Tauri update + `cn`, goes to `dev` before the 1.1.0 release.
- Deleted 2026-11-05: `review-fixes`, `github-pages` (both merged), `live-wall`
  (old player, kept as tag `archive/live-wall` → 14f72eb).

## Sessions

### 2026-11-06 — version scripts, Prettier (host EMON)

- On `deps-update`. `scripts/sync-version.mjs`: the default run
  (`yarn version:sync`, `version:patch|minor|major`) now writes only
  `Cargo.toml` / `Cargo.lock`, not `website/index.html`; the Pages deploy
  writes the released version with `--website --version` (unchanged).
  `bump-version.js` → `bump-version.mjs`. Tested in a scratch copy: bump,
  sync, `--check`, `--website`, bad usage.
- You ran Prettier on the whole repo: `yarn prettier` is now clean.
- `vite.config.ts` `build` block (`target` safari13 / chrome105, `minify`,
  `sourcemap`) and `envPrefix` are needed and stay: the Tauri CLI sets
  `TAURI_ENV_PLATFORM` / `TAURI_ENV_DEBUG` itself when it runs `yarn dev` /
  `yarn build`. `.env.example` only described them; nothing read it.

### 2026-11-06 — other npm updates, ESLint 10 (host EMON)

- On `deps-update`. Updated after checking peer dependencies: React /
  react-dom 19.3.1 (+ `@types/react(-dom)` 19.3.0), lucide-react 1.69.0,
  `@tanstack/react-virtual` 3.14.26 (still pinned exact), semver 7.8.6, Vite
  8.3.6, `@vitejs/plugin-react` 6.2.1, Tailwind 4.3.4, Babel 8.1.0 /
  eslint-parser 8.0.7, `@types/node` 26.6.6, globals 17.14.0, prettier 3.9.10,
  eslint-plugin-react-refresh 0.5.7.
- **ESLint 9 → 10** (`eslint` 10.14.0, `@eslint/js` 10.0.1): every plugin
  supports 10 and the config was already flat config, so no config change. Lint
  still covers all 31 files (checked with a probe file).
- Fixed `src/components/icons.tsx`: `stroke-width` etc. → `strokeWidth`
  (React warned "Invalid DOM property" on the About page).
- Checked: `yarn build`, lint, frozen install, `cargo check`; every page in the
  browser dev mock. Known, not from this change: the Gallery page crashes in
  the browser mock only (`src/dev/browserMock.ts` has no `search_gallery`);
  in the real app the call goes to Rust.

### 2026-11-05 — Tauri update, `cn` package, clean-up (host EMON)

- Branch `deps-update` from `dev`. Local clean-up first: deleted local
  `live-wall`, `review-fixes` (gone on GitHub); `main`, `dev` fast-forwarded.
- **Tauri updated** (only Tauri and its plugins, other packages left as they
  are): JS `@tauri-apps/api` 2.12.4, `cli` 2.12.4, `plugin-autostart` 2.7.0,
  `plugin-dialog` 2.8.4, `plugin-log` 2.10.1, `plugin-opener` 2.7.2. Rust
  `tauri` 2.12.5, `tauri-build` 2.7.4, same plugin versions as JS,
  `single-instance` 2.5.5 (wry 0.57.1). `tauri info` shows JS and Rust match.
  `tauri-plugin-aptabase` git tag 1.2.0 → 1.3.0 (system proxy fix; new crash
  reporting is opt-in and not turned on).
- **`cn` package** (shadcn, replaces `clsx` + `tailwind-merge`): components
  import `cn` from `"cn"`; our own `cn()` in `src/lib/utils.ts` removed.
  Checked: all 276 class strings in `src/`, 76,176 pairs, give the same result
  as `twMerge(clsx(...))`.
- **Removed:** `esbuild` (not used, Vite 8 does not need it), `clsx`,
  `tailwind-merge`, `.env.example` (only listed variables the Tauri CLI sets;
  Vite does not load `.env` into `vite.config.ts`). `tsconfig.node.json` was
  never checked: `yarn build` now type-checks `vite.config.ts` with it.
- Checked: `yarn build`, eslint, `yarn install --frozen-lockfile`, `cargo fmt`,
  clippy `-D warnings`, 32 app tests. Not checked: the running app (your
  check), CI on Windows / macOS.
- Known, not from this change: Vite warns about top-level await in
  `src/main.tsx` (dev-only browser mock); Prettier flagged 5 untouched files
  (fixed 2026-11-06).
- Your local `.env` (untracked) only has unused variables; you can delete it.

### 2026-11-05 — 1.1.0 bumped, PR #9 into `dev` (host EMON)

- `yarn version:minor` → 1.1.0 (package.json, Cargo.toml / .lock, website)
  (326bb86). Checked: `yarn build`, eslint, fmt, clippy `-D warnings`, 32 app +
  15 engine tests.
- PR #9 `live-engine` → `dev` "Release 1.1.0: live wallpapers on GNOME"
  (44 commits), mergeable; CI starting.

### 2026-11-05 — README for live wallpapers (host EMON)

- `README.md`: intro and feature list mention live wallpapers; new section
  "Live wallpapers" (how it works, support table: GNOME on Wayland yes,
  Windows / macOS coming, KDE / X11 not yet; GNOME one-time helper setup;
  FFmpeg for `.deb` vs AppImage); `live/` row in the storage table; building
  from source: engine built by `build.rs`, `node scripts/fetch-ffmpeg.mjs` on
  Windows / macOS; bundled FFmpeg license note; limitation "GNOME on Wayland,
  same video on every monitor".
- `scripts/fetch-ffmpeg.mjs`: comments no longer say the repo is private
  (the token is optional).

### 2026-11-05 — website for 1.1.0 (live wallpaper) on `live-engine` (host EMON)

- Vercel project and secrets deleted by you (only `APTABASE_APP_KEY` left).
- `website/`: meta / Open Graph / Twitter / structured-data text mention
  live wallpapers on GNOME; hero lede and scenes are now Live, Gallery,
  Library (captions in `main.js`); new featured card "Live wallpapers"
  (GNOME on Wayland now, Windows / macOS coming), Gallery card mentions the
  full-screen preview; new how-to step 3 "Play a video as a live wallpaper"
  (steps now 1–6); `.deb` / AppImage cards mention FFmpeg.
- `assets/screenshots/live.png` (1000×700): taken from the dev app through
  the WebKit inspector (`Page.snapshotRect`), so no OS window frame.
- `sync-version.mjs` now also updates `"softwareVersion"` in the
  structured data (was stuck at 1.0.2; set to 1.0.6).
- `.claude/launch.json`: `website` static server on port 4180.
- Checked in the browser pane: desktop and 375 px phone width, no horizontal
  scroll, no console errors.
- Found while doing it: you had installed the 1.0.6 `.deb` (18:05), which
  replaced the `live-engine` build, so no live wallpaper plays here now.

### 2026-11-05 — 1.0.6 released (host EMON)

- Draft deleted, `v1.0.6` tagged on 56c878c and pushed. Release run
  42024407592: all builds green, release published as Latest with the same 5
  files; the website job started Pages (workflow_dispatch, green).
- Checked: GitHub latest-release API returns v1.0.6; the site says
  "Download Wallflow 1.0.6" and all 5 download links redirect (302).

### 2026-11-05 — 1.0.6 rehearsal: draft release (host EMON)

- Release workflow run by hand on `main` 56c878c (run 42017083330): all four
  builds green, website job skipped, draft `Wallflow v1.0.6` with
  `Wallflow_1.0.6_amd64.deb` (5.7 MB), `_amd64.AppImage` (84 MB),
  `_x64-setup.exe` (4.1 MB), `_aarch64.dmg` (7.5 MB), `_x64.dmg` (7.8 MB):
  the names the website links to.
- `.deb` checked without installing: version 1.0.6, depends
  appindicator / webkit2gtk-4.1 / gtk3, needs GLIBC 2.34, links to the Pages
  URL, no old domain. AppImage runs (`--appimage-version`).
- Not installed here: the machine runs the `live-engine` build.
- Website for live wallpaper: decided to ship it with 1.1.0, not 1.0.6.

### 2026-11-05 — gallery preview "Could not load the full image" fixed (host EMON)

- Cause: `index.html` has its own CSP `<meta>`, enforced together with the
  CSP from `tauri.conf.json`. The preview (9d7fa5a) added `w.wallhaven.cc` to
  `tauri.conf.json` only, so the meta still refused every full image (only
  `th.wallhaven.cc` thumbnails were allowed). Added `w.wallhaven.cc` to the
  meta's `img-src` (030e86d). Only on `live-engine` (`main`/`dev` have no preview yet).
- Checked in the real dev app through WebKit's remote inspector: before, the
  console said "Refused to load https://w.wallhaven.cc/full/… img-src"; after,
  the full image loads (200, no CSP error).
- When changing the CSP: change **both** `index.html` and `tauri.conf.json`.

### 2026-11-05 — PR #8 merged, branches cleaned up (host EMON)

- You merged PR #8; CI and the Website deploy on `main` passed (runs
  42009086688, 42009086648).
- Branches: deleted `github-pages`, `review-fixes` (fully in `main`) and
  `live-wall` (tag `archive/live-wall` first). `dev` fast-forwarded to `main`
  (56c878c). `main` merged into `live-engine`.
- Summary for the next session written at the top.

### 2026-11-05 — no custom domain; 1.0.6 prepared (host EMON)

- You dropped `wallflow.efadhemon.dev`: the site is
  `https://efadhemon.github.io/wallflow/` (58b871d): meta / canonical /
  structured-data URLs, About and update-banner links, opener allowlist;
  `404.html` has `<base href="/wallflow/">`; dev CSP leftovers gone; repo
  website field set to the Pages URL. `api/version.json` removed again (only
  1.0.5 apps read it, from the old domain).
- 1.0.6 prepared on `github-pages` (PR #8 retitled "Release 1.0.6"): `dev` and
  `review-fixes` merged in cleanly, `yarn version:patch` (e06534c).
- You found the old domain still in your checkout: the change was only on
  `github-pages`. Merged `github-pages` into `live-engine` (conflicts:
  `.gitignore`, CSP — kept `w.wallhaven.cc` for the gallery preview). Now no
  `wallflow.efadhemon.dev`, Vercel or `VITE_VERSION_API_URL` outside this log;
  FFmpeg fetch steps and Pages / draft-release workflow both present.
  tsc, eslint, clippy, 32 app + engine tests pass. Worktree `.worktrees/pages`
  removed (nothing unpushed).

### 2026-11-05 — repository public: website to GitHub Pages, release plan (host EMON)

- Branch `github-pages` (from `main`, worktree `.worktrees/pages`), PR #8:
  `pages.yml` deploys `website/` to Pages (on website changes, by hand, after
  each tagged release) with download links to the latest published release;
  update check via GitHub's latest-release API (needs a User-Agent: 403
  without); `api/version.json` kept for apps up to 1.0.5; Vercel files,
  `deploy-website.yml`, the Vercel release job, installer staging and
  `VITE_VERSION_API_URL` removed; CSP leftovers removed; CI on pull requests;
  manual release runs are drafts. Commits 0ec1d76, 9bd1b76, 94ed849.
- Pages enabled with GitHub Actions as source (`efadhemon.github.io/wallflow`,
  deploys allowed from `main`). Setting the custom domain was not allowed from
  here (DNS / domain change): yours.
- Checked: tsc, eslint, clippy, 20 tests; API returns v1.0.5; `.deb`,
  AppImage and `.exe` release links redirect (302). The two `.dmg` links gave
  504 from this network although the API lists them: check in a browser.

### 2026-11-05 — FFmpeg release published, sidecars wired in (host EMON)

- FFmpeg workflow: `workflow_dispatch` needs the file on `main`, so it now also
  runs on pushes that change the FFmpeg build (an existing release is skipped).
  Run 41982694843: all three builds green, pre-release `ffmpeg-n9.0.2-wf1`.
  Archives: Windows 7.7 MB, macOS arm64 5.6 MB, x86_64 6.6 MB (ffmpeg +
  ffprobe, compressed). Windows exe imports only system DLLs (kernel32,
  msvcrt, ole32, bcrypt, shell32, user32) and has `h264_mf`; macOS smoke test
  found `h264_videotoolbox`.
- The repository is **private**: release files 404 without a token.
  `fetch-ffmpeg.mjs` now downloads through the API with `GH_TOKEN` /
  `GITHUB_TOKEN` / `gh auth token`; CI and release pass `github.token`.
- 2.4: `tauri.windows.conf.json` / `tauri.macos.conf.json` add `ffmpeg` and
  `ffprobe` to `externalBin` and ship `licenses/FFmpeg-LICENSE.txt` +
  `FFmpeg-BUILD.txt`; CI (macOS, Windows) and release fetch before building.
- 2.6: About → "Third-party software": FFmpeg, LGPL, link to FFmpeg's own
  source at the pinned commit (users cannot open the private release).
- 3.4 `direct` backend (4fca41d), see the commit; 3.3 moved into W1.
- CI run 41983441060 (3bf6457): green on all three; the Windows and macOS jobs
  fetched FFmpeg from the release with the token.

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

- **LGPL source:** settled by the repository going public: the
  `ffmpeg-n9.0.2-wf1` release (build script, configure line) is public and
  links FFmpeg's source at the pinned commit. Attaching the FFmpeg source
  tarball to that release would make it complete on our side.
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
- **Later: ESLint with `typescript-eslint` instead of Babel.** Now
  `@babel/eslint-parser` (+ its peer `@babel/core`) parses TS/TSX in
  `eslint.config.mjs`, only because `typescript-eslint` does not support
  TypeScript 7 yet (8.72.1 needs `typescript <6.1.0`, we use 7.0.2). Babel has
  no types, so `no-undef`, `no-unused-vars`, `no-redeclare` are off (`tsc`
  `noUnusedLocals` / `noUnusedParameters` cover unused code). When
  `typescript-eslint` supports TS 7 (check `npm view typescript-eslint
peerDependencies`): add it, remove both Babel packages, turn on its
  recommended (type-aware) rules.

## Next step

See **Summary — start here** at the top.

## Release plan

The repository is public (2026-11-05). Website: GitHub Pages at
**https://efadhemon.github.io/wallflow/** (no custom domain;
`.github/workflows/pages.yml`), download buttons and the in-app update check
read GitHub releases. A **manual** run of `release.yml` makes a **draft**
release (rehearsal: test the installers, then publish the draft by hand and run
the Website workflow, or delete it and push the tag); a **tag push** publishes
and refreshes the website. Apps up to 1.0.5 look for updates at
`wallflow.efadhemon.dev/api/version.json`; with that domain gone they show no
update banner, so their users update by hand once.

**1.0.6 — the public, low-risk release (from `main`)** — **released 2026-11-05** (tag `v1.0.6`), prepared in PR #8

- Branch `github-pages`: Pages move, `dev` (titlebar, gallery virtualization)
  and `review-fixes` (4193a7a) merged in, version bumped (e06534c). No live
  wallpaper. Checked: tsc, eslint, `yarn build`, fmt, clippy, 25 tests.
- Merge → tag `v1.0.6`. Proves the new pipeline: release assets, Pages
  refresh, 1.0.6 asks GitHub for updates.

**1.1.0 — live wallpapers on Linux (GNOME) (`live-engine`)**

- Merge `main` into `live-engine` (workflow conflicts: keep the FFmpeg fetch
  steps from `live-engine`, the Pages / no-Vercel changes from `main`).
- Rehearsal: manual release run → draft. Check on real machines: `.deb` on
  Ubuntu 24.04 (helper install from the Live page, log out / in, play), the
  AppImage (FFmpeg banner when the system has none), Windows and macOS install
  and run with the Live page saying "coming soon", and their installer sizes
  (step 2.7).
- Release notes: GNOME only; KDE, X11, Windows, macOS later; FFmpeg LGPL
  (source: release `ffmpeg-n9.0.2-wf1`, public now).
- `live-engine` → `dev` → `main`, tag `v1.1.0`; drop `live-wall`.

**1.2.0 — Windows live wallpaper** (Plan step 4, needs a Windows PC)
**1.3.0 — macOS live wallpaper** (Plan step 5, needs a Mac)
**Later — KDE / wlroots, X11, zero-copy** (Plan step 6)

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

### Step 2 — FFmpeg bundle (done except 2.7: real installer sizes come with the first Windows / macOS build)

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

### Step 3 — W0, common groundwork (3.1 980dcc6, 3.2 82c6419, 3.4 4fca41d done; 3.3 moved into W1)

- 3.1 `prepare` picks the encoder from `ffmpeg -encoders`: `libx264` (CRF 20)
  → `h264_mf` / `h264_videotoolbox` (bitrate ≈ 0.1 bit per pixel per frame,
  e.g. 1080p30 ≈ 6 Mbit/s) → error. Pure function, unit tested with recorded
  encoder lists.
- 3.2 Engine prints JSON lines on stdout while playing, with `--events`: `ready` (screens),
  `state` (`playing` / `paused`, why), `error`. The GNOME shim ignores them.
- 3.3 (done together with 4.1 / 4.2: without a platform that sends cover
  signals it would only be dead code) Engine `platform` module: `attach_background(window, monitor)` and a
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
