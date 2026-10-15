#!/bin/sh
# Adds Prism to your app launcher and taskbar with its icon, for the
# current user only (no admin rights needed). Run it again after moving
# the project. Remove with: scripts/install-desktop.sh --remove
set -e
here=$(cd "$(dirname "$0")/.." && pwd)
data=${XDG_DATA_HOME:-$HOME/.local/share}
icons="$data/icons/hicolor"
entry="$data/applications/prism-rgb.desktop"

if [ "$1" = "--remove" ]; then
  rm -f "$entry"
  for s in 32 64 128 256 512; do rm -f "$icons/${s}x${s}/apps/prism-rgb.png"; done
  rm -f "$icons/scalable/apps/prism-rgb.svg"
  echo "Removed Prism from the app launcher."
  exit 0
fi

# Prefer an optimized build if there is one.
bin="$here/target/release/prism-rgb"
[ -x "$bin" ] || bin="$here/target/debug/prism-rgb"
if [ ! -x "$bin" ]; then
  echo "Build Prism first (npm run build, or cargo build -p prism-rgb)." >&2
  exit 1
fi

src="$here/src-tauri/icons"
install -Dm644 "$src/32x32.png"       "$icons/32x32/apps/prism-rgb.png"
install -Dm644 "$src/64x64.png"       "$icons/64x64/apps/prism-rgb.png"
install -Dm644 "$src/128x128.png"     "$icons/128x128/apps/prism-rgb.png"
install -Dm644 "$src/128x128@2x.png"  "$icons/256x256/apps/prism-rgb.png"
install -Dm644 "$src/icon.png"        "$icons/512x512/apps/prism-rgb.png"
install -Dm644 "$src/prism.svg"       "$icons/scalable/apps/prism-rgb.svg"

# The file name and StartupWMClass match the window's app id ("prism-rgb"),
# which is how the taskbar knows which icon to show.
mkdir -p "$(dirname "$entry")"
cat > "$entry" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Prism
GenericName=Corsair device control
Comment=Lighting, key bindings and DPI for Corsair keyboards, mice and fans
Exec=$bin
Icon=prism-rgb
StartupWMClass=prism-rgb
Categories=Settings;HardwareSettings;Utility;
Keywords=corsair;rgb;lighting;keyboard;mouse;fans;macro;
Terminal=false
DESKTOP

# Let the desktop notice the new icon and entry.
command -v update-desktop-database >/dev/null && update-desktop-database "$data/applications" 2>/dev/null || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$icons" 2>/dev/null || true
command -v kbuildsycoca6 >/dev/null && kbuildsycoca6 >/dev/null 2>&1 || true
echo "Prism added to your app launcher: $entry"
