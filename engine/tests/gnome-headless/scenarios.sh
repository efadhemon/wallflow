#!/bin/bash
# Diagnostic: does the video clone survive what happens in the overview
# (window drags, moving a window, a wallpaper change)? SCRIPT=scenarios.sh run.sh
set -uo pipefail
unset DISPLAY WAYLAND_DISPLAY XDG_SESSION_TYPE
dconf write /org/gnome/shell/enabled-extensions "['wallflow-live@com.efadhemon.wallflow','probe@wallflow-test'${EXTRA_EXTENSIONS:+,$EXTRA_EXTENSIONS}]"
dconf write /org/gnome/shell/disable-user-extensions false
dconf write /org/gnome/shell/welcome-dialog-last-shown-version "'999'"
dconf write /org/gnome/mutter/dynamic-workspaces false
dconf write /org/gnome/desktop/wm/preferences/num-workspaces 3
gnome-shell --headless --wayland --no-x11 --virtual-monitor 1920x1080 \
  --wayland-display wallflow-scen-$$ > "$ROOT/shell.log" 2>&1 &
SHELL_PID=$!
trap 'kill $SHELL_PID 2>/dev/null; wait $SHELL_PID 2>/dev/null' EXIT
probe() { gdbus call --session --dest test.Probe --object-path /test/Probe --method "test.Probe.$1" "${@:2}"; }
shim() { gdbus call --session --dest com.efadhemon.Wallflow.LiveShim --object-path /com/efadhemon/Wallflow/LiveShim --method "com.efadhemon.Wallflow.LiveShim.$1" "${@:2}" >/dev/null; }
for _ in $(seq 1 60); do probe Report >/dev/null 2>&1 && break; sleep 0.5; done
clones() { probe Report | grep -o '"overviewClones":[0-9]*' | grep -o '[0-9]*$'; }
probe Open "['gnome-text-editor']" >/dev/null; sleep 3
shim Start "['$ENGINE', '$VIDEO', '--background']"; sleep 4
probe Overview true >/dev/null; sleep 2
echo "opened overview:        clones=$(clones)"
for x in 1880 1910; do echo "drop target at side card x=$x: $(probe DropTargetAt $x.0 540.0)"; done
for s in dragBegin dragCancel dragBegin dragEnd moveWindow; do probe Scenario "'$s'" >/dev/null; sleep 1.5; echo "after $s:$(printf '%*s' $((14-${#s})) '') clones=$(clones)"; done
dconf write /org/gnome/desktop/background/picture-uri "'file:///usr/share/backgrounds/warty-final-ubuntu.png'"; sleep 2
echo "after wallpaper change: clones=$(clones)"
probe Shot "'$ROOT/after.png'" >/dev/null
probe Overview false >/dev/null; sleep 2; probe Overview true >/dev/null; sleep 2
echo "reopened overview:      clones=$(clones)"
shim Stop
