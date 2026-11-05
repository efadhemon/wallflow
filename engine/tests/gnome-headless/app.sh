#!/bin/bash
# The Wallflow app (debug build) inside the headless shell: a live wallpaper
# chosen before restarts by itself through the shim, the image rotation stays
# out of the way, and locking does not end it. Run via SCRIPT=app.sh run.sh.
set -uo pipefail
unset DISPLAY WAYLAND_DISPLAY XDG_SESSION_TYPE
[ -x "$APP" ] || { echo "build the app first: (cd src-tauri && cargo build)"; exit 1; }
dconf write /org/gnome/shell/enabled-extensions "['wallflow-live@com.efadhemon.wallflow','probe@wallflow-test']"
dconf write /org/gnome/shell/disable-user-extensions false
dconf write /org/gnome/shell/welcome-dialog-last-shown-version "'999'"

# A library with one prepared video, selected as the live wallpaper.
LIB="$XDG_DATA_HOME/com.efadhemon.wallflow/live/e2e"
mkdir -p "$LIB" "$XDG_CONFIG_HOME/com.efadhemon.wallflow"
"$ENGINE" prepare "$VIDEO" --out "$LIB/video.mp4" --poster "$LIB/poster.jpg" --width 1280 --height 720 >/dev/null 2>&1
cat > "$LIB/meta.json" <<JSON
{"id":"e2e","name":"E2E clip","width":1280,"height":720,"fps":30,"duration":5,"addedAt":1}
JSON
IMAGES="$ROOT/images"; mkdir -p "$IMAGES"; cp /usr/share/backgrounds/*.png "$IMAGES/" 2>/dev/null
cat > "$XDG_CONFIG_HOME/com.efadhemon.wallflow/config.json" <<JSON
{"version":1,"folders":[{"path":"$IMAGES","recursive":false}],"intervalSeconds":5,"changeOnStartup":true,
 "autostart":false,"startMinimized":true,"liveVideo":"e2e"}
JSON

DISPLAY_NAME=wallflow-app-$$
gnome-shell --headless --wayland --no-x11 --virtual-monitor 1920x1080 \
  --wayland-display $DISPLAY_NAME > "$ROOT/shell.log" 2>&1 &
SHELL_PID=$!
trap 'kill $APP_PID $SHELL_PID 2>/dev/null; wait 2>/dev/null' EXIT
probe() { gdbus call --session --dest test.Probe --object-path /test/Probe --method "test.Probe.$1" "${@:2}"; }
prop() { gdbus call --session --dest com.efadhemon.Wallflow.LiveShim --object-path /com/efadhemon/Wallflow/LiveShim --method org.freedesktop.DBus.Properties.Get com.efadhemon.Wallflow.LiveShim "$1" 2>/dev/null | grep -o 'true\|false'; }
for _ in $(seq 1 60); do probe Report >/dev/null 2>&1 && break; sleep 0.5; done
failures=0
check() { if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2', want '$3'"; failures=$((failures+1)); fi; }

uri_before=$(gsettings get org.gnome.desktop.background picture-uri)
WAYLAND_DISPLAY=$DISPLAY_NAME XDG_SESSION_TYPE=wayland XDG_CURRENT_DESKTOP=GNOME \
  "$APP" --minimized > "$ROOT/app.log" 2>&1 &
APP_PID=$!

for _ in $(seq 1 40); do [ "$(prop Running)" = true ] && break; sleep 0.5; done
check "the app restores the live wallpaper at startup" "$(prop Running)" true

sleep 12   # more than two rotation intervals
check "the image rotation leaves the wallpaper alone" "$(gsettings get org.gnome.desktop.background picture-uri)" "$uri_before"

probe Lock true >/dev/null; sleep 3
probe Lock false >/dev/null; sleep 3
check "still playing after lock and unlock" "$(prop Running):$(prop Paused)" "true:false"

# The engine dies (shell restart, crash): the app starts it again.
engine_pid() { pgrep -f -- "--background" | while read -r pid; do grep -qz "$LIB" /proc/$pid/cmdline 2>/dev/null && echo "$pid"; done | head -1; }
before=$(engine_pid)
check "found the engine process" "$([ -n "$before" ] && echo yes)" yes
kill "$before" 2>/dev/null
for _ in $(seq 1 20); do [ "$(prop Running)" = false ] && break; sleep 0.25; done
check "the shim notices the engine died" "$(prop Running)" false
for _ in $(seq 1 40); do [ "$(prop Running)" = true ] && break; sleep 0.5; done
after=$(engine_pid)
check "the app restarts a live wallpaper that died" "$(prop Running):$([ -n "$after" ] && [ "$after" != "$before" ] && echo new)" "true:new"

grep -E "live wallpaper|wallflow_lib::live" "$ROOT/app.log" | tail -4
[ $failures -eq 0 ] && echo "all checks passed" || { echo "$failures check(s) failed"; exit 1; }
