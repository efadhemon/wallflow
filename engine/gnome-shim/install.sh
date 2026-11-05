#!/bin/bash
# Installs the GNOME shim for the current user and turns off the old
# `live-wallpaper@com.efadhemon.wallflow` extension, which decoded video inside
# gnome-shell. On Wayland, GNOME only loads a newly installed extension after you
# log out and back in.
#
#   engine/gnome-shim/install.sh            # install (symlink, for development)
#   engine/gnome-shim/install.sh --remove   # undo: remove it, re-enable the old one
set -euo pipefail

UUID=wallflow-live@com.efadhemon.wallflow
OLD=live-wallpaper@com.efadhemon.wallflow
SRC=$(cd "$(dirname "$0")" && pwd)
DEST=${XDG_DATA_HOME:-$HOME/.local/share}/gnome-shell/extensions/$UUID

enabled() { gsettings get org.gnome.shell enabled-extensions; }
set_enabled() { gsettings set org.gnome.shell enabled-extensions "$1"; }

# Adds or removes one UUID in the enabled list without touching the others.
edit_list() {
  python3 - "$1" "$2" "$(enabled)" <<'EOF'
import ast, sys
action, uuid, raw = sys.argv[1:]
items = list(ast.literal_eval(raw.replace('@as ', '')))
if action == 'add' and uuid not in items:
    items.append(uuid)
if action == 'remove':
    items = [i for i in items if i != uuid]
print(repr(items))
EOF
}

if [ "${1:-}" = "--remove" ]; then
  rm -f "$DEST"
  set_enabled "$(edit_list remove "$UUID")"
  if [ -d "${XDG_DATA_HOME:-$HOME/.local/share}/gnome-shell/extensions/$OLD" ]; then
    set_enabled "$(edit_list add "$OLD")"
  fi
  echo "Removed $UUID. Log out and back in to finish."
  exit 0
fi

mkdir -p "$(dirname "$DEST")"
ln -sfn "$SRC" "$DEST"
gnome-extensions disable "$OLD" 2>/dev/null || true
set_enabled "$(edit_list remove "$OLD")"
set_enabled "$(edit_list add "$UUID")"
echo "Installed $UUID (old $OLD turned off). Log out and back in, then run:"
echo "  gdbus call --session --dest com.efadhemon.Wallflow.LiveShim \\"
echo "    --object-path /com/efadhemon/Wallflow/LiveShim \\"
echo "    --method com.efadhemon.Wallflow.LiveShim.Start \"['$(cd "$SRC/.." && pwd)/target/release/wallflow-live', 'VIDEO.mp4', '--background']\""
