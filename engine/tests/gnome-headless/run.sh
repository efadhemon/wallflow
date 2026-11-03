#!/bin/bash
# End-to-end test of the engine and the GNOME shim in a throwaway, headless
# gnome-shell. Nothing touches the real session: the shell gets its own D-Bus,
# Wayland socket and XDG directories (dconf included) under a temp dir.
#
#   tests/gnome-headless/run.sh [VIDEO]
#
# Checks: the window becomes a hidden desktop window stacked right above the
# background, app windows stack above it, and the overview / a maximized window
# pause playback. Screenshots are left in the temp dir for a visual check.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
ENGINE_DIR=$(cd "$HERE/../.." && pwd)
VIDEO=${1:-$(ls ~/Videos/*.mp4 | head -1)}
ENGINE=$ENGINE_DIR/target/release/wallflow-live
[ -x "$ENGINE" ] || (cd "$ENGINE_DIR" && cargo build --release)

ROOT=$(mktemp -d -t wallflow-gnome-XXXX)
EXT=$ROOT/data/gnome-shell/extensions
mkdir -p "$EXT" "$ROOT/config" "$ROOT/cache" "$ROOT/state"
ln -s "$ENGINE_DIR/gnome-shim" "$EXT/wallflow-live@com.efadhemon.wallflow"
ln -s "$HERE/probe@wallflow-test" "$EXT/probe@wallflow-test"
echo "test dir: $ROOT"

# The XDG variables must be set before dbus-run-session, so every service the
# private bus starts (dconf-service above all) writes into the temp dir.
exec env XDG_CONFIG_HOME="$ROOT/config" XDG_DATA_HOME="$ROOT/data" \
  XDG_CACHE_HOME="$ROOT/cache" XDG_STATE_HOME="$ROOT/state" \
  ROOT="$ROOT" ENGINE="$ENGINE" VIDEO="$VIDEO" \
  dbus-run-session -- "$HERE/session.sh"
