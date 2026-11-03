#!/bin/bash
# Runs inside the private session bus started by run.sh.
set -uo pipefail
unset DISPLAY WAYLAND_DISPLAY XDG_SESSION_TYPE
dconf write /org/gnome/shell/enabled-extensions "['wallflow-live@com.efadhemon.wallflow','probe@wallflow-test'${EXTRA_EXTENSIONS:+,$EXTRA_EXTENSIONS}]"
dconf write /org/gnome/shell/disable-user-extensions false
dconf write /org/gnome/shell/welcome-dialog-last-shown-version "'999'"

monitor_args=()
for size in $MONITORS; do monitor_args+=(--virtual-monitor "$size"); done
gnome-shell --headless --wayland --no-x11 "${monitor_args[@]}" \
  --wayland-display wallflow-test-$$ > "$ROOT/shell.log" 2>&1 &
SHELL_PID=$!
trap 'kill $SHELL_PID 2>/dev/null; wait $SHELL_PID 2>/dev/null' EXIT

probe() { gdbus call --session --dest test.Probe --object-path /test/Probe --method "test.Probe.$1" "${@:2}"; }
shim() { gdbus call --session --dest com.efadhemon.Wallflow.LiveShim --object-path /com/efadhemon/Wallflow/LiveShim --method "com.efadhemon.Wallflow.LiveShim.$1" "${@:2}"; }
paused() { gdbus call --session --dest com.efadhemon.Wallflow.LiveShim --object-path /com/efadhemon/Wallflow/LiveShim --method org.freedesktop.DBus.Properties.Get com.efadhemon.Wallflow.LiveShim Paused | grep -o 'true\|false'; }
for _ in $(seq 1 60); do probe Report >/dev/null 2>&1 && break; sleep 0.5; done

failures=0
check() { if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2', want '$3'"; failures=$((failures+1)); fi; }

shim Start "['$ENGINE', '$VIDEO', '--background']" >/dev/null
sleep 5
check "plays on an uncovered desktop" "$(paused)" false
report=$(probe Report)
check "desktop window above background" "$(echo "$report" | grep -o '"windowGroupOrder":\["BACKGROUND","wallflow-live' | wc -l)" 1
check "every video window is DESKTOP type" "$(echo "$report" | grep -o '"cls":"wallflow-live","title":"[^"]*","type":[0-9]*' | grep -vc '"type":1$')" 0
check "hidden from Alt-Tab" "$(echo "$report" | grep -c '"tabList":\[[^]]*wallflow-live')" 0
monitor_count=$(echo $MONITORS | wc -w)
check "one window per monitor" "$(echo "$report" | grep -o '"cls":"wallflow-live"' | wc -l)" "$monitor_count"
check "each window on its own monitor" "$(echo "$report" | grep -o '"cls":"wallflow-live"[^}]*"mon":[0-9]*' | grep -o '"mon":[0-9]*' | sort -u | wc -l)" "$monitor_count"
probe Shot "'$ROOT/desktop.png'" >/dev/null

probe Overview true >/dev/null; sleep 2
check "overview pauses" "$(paused)" true
probe Overview false >/dev/null; sleep 2
check "leaving the overview resumes" "$(paused)" false

probe Open "['gnome-text-editor']" >/dev/null; sleep 4
order=$(probe Report | grep -o '"windowGroupOrder":[^]]*]')
echo "     stacking: $order"
# Video right above the background; anything else (desktop icons, apps) above it.
check "app window stacks above the video" "$(echo "$order" | grep -oE '\["BACKGROUND",("wallflow-live[^"]*",)+.*"org.gnome.TextEditor' | wc -l)" 1
probe Shot "'$ROOT/with-window.png'" >/dev/null
probe Max "'org.gnome.TextEditor'" true >/dev/null; sleep 2
# With more monitors, the others still show the desktop, so playback goes on.
if [ "$monitor_count" -eq 1 ]; then want=true; else want=false; fi
check "a maximized window pauses only if it hides every desktop" "$(paused)" "$want"
probe Max "'org.gnome.TextEditor'" false >/dev/null; sleep 2
check "unmaximizing resumes" "$(paused)" false

shim Stop >/dev/null; sleep 1
check "engine exits on Stop" "$(pgrep -fc "$ENGINE" || true)" 0

grep -E "wallflow_live" "$ROOT/shell.log" | tail -4
[ $failures -eq 0 ] && echo "all checks passed" || { echo "$failures check(s) failed"; exit 1; }
