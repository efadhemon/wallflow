#!/bin/bash
# CPU cost of the live wallpaper in each desktop state, inside the headless
# shell started by run.sh (SCRIPT=bench.sh). Prints % of one core for the
# shell and for the engine + its ffmpeg.
set -uo pipefail
unset DISPLAY WAYLAND_DISPLAY XDG_SESSION_TYPE
OLD=live-wallpaper@com.efadhemon.wallflow
OLD_DIR=${OLD_EXTENSION_DIR:-$HOME/.local/share/gnome-shell/extensions/$OLD}
# For comparison, also load the old in-shell player when it is installed.
[ -d "$OLD_DIR" ] && ln -sfn "$OLD_DIR" "$XDG_DATA_HOME/gnome-shell/extensions/$OLD"
dconf write /org/gnome/shell/enabled-extensions "['wallflow-live@com.efadhemon.wallflow','probe@wallflow-test','$OLD']"
dconf write /org/gnome/shell/disable-user-extensions false
dconf write /org/gnome/shell/welcome-dialog-last-shown-version "'999'"
gnome-shell --headless --wayland --no-x11 --virtual-monitor 1920x1080 \
  --wayland-display wallflow-bench-$$ > "$ROOT/shell.log" 2>&1 &
SHELL_PID=$!
trap 'kill $SHELL_PID 2>/dev/null; wait $SHELL_PID 2>/dev/null' EXIT

probe() { gdbus call --session --dest test.Probe --object-path /test/Probe --method "test.Probe.$1" "${@:2}" >/dev/null; }
shim() { gdbus call --session --dest com.efadhemon.Wallflow.LiveShim --object-path /com/efadhemon/Wallflow/LiveShim --method "com.efadhemon.Wallflow.LiveShim.$1" "${@:2}" >/dev/null; }
for _ in $(seq 1 60); do probe Report 2>/dev/null && break; sleep 0.5; done

hz=$(getconf CLK_TCK)
ticks() { local t=0; for p in "$@"; do [ -r /proc/$p/stat ] && t=$((t + $(awk '{print $14+$15}' /proc/$p/stat))); done; echo $t; }
engine_pids() { local e; e=$(pgrep -f "^$ENGINE " | head -1); [ -n "$e" ] && echo "$e $(pgrep -P "$e")"; }
measure() { # label
  local ep; ep=$(engine_pids)
  local s0 e0 t0; s0=$(ticks $SHELL_PID); e0=$(ticks $ep); t0=$(date +%s.%N)
  sleep 8
  local s1 e1 t1; s1=$(ticks $SHELL_PID); e1=$(ticks $ep); t1=$(date +%s.%N)
  printf "%-28s shell %5.1f%%   engine+ffmpeg %5.1f%%\n" "$1" \
    "$(echo "($s1-$s0)*100/$hz/($t1-$t0)" | bc -l)" "$(echo "($e1-$e0)*100/$hz/($t1-$t0)" | bc -l)"
}

sleep 3
measure "static desktop"
shim Start "['$ENGINE', '$VIDEO', '--background']"; sleep 4
measure "live wallpaper playing"
probe Overview true; sleep 1
measure "overview open (paused)"
probe Overview false; sleep 2
probe Open "['gnome-text-editor']"; sleep 4
measure "app window on top"
probe Max "'org.gnome.TextEditor'" true; sleep 1
measure "app maximized (paused)"
shim Stop
probe Max "'org.gnome.TextEditor'" false

if gdbus introspect --session --dest com.efadhemon.Wallflow.Live --object-path /com/efadhemon/Wallflow/Live >/dev/null 2>&1; then
  echo "--- old in-shell player (live-wall branch), same video"
  old() { gdbus call --session --dest com.efadhemon.Wallflow.Live --object-path /com/efadhemon/Wallflow/Live --method "com.efadhemon.Wallflow.Live.$1" "${@:2}" >/dev/null; }
  old SetVideo "'$VIDEO'" "'fill'" true; sleep 4
  measure "old: playing, app on top"
  probe Overview true; sleep 1
  measure "old: overview open"
  probe Overview false; sleep 1
  old Stop
fi
