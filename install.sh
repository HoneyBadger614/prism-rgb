#!/bin/sh
# Prism installer: builds Prism on this computer and installs it for you.
#
#   ./install.sh               install (or update)
#   ./install.sh --uninstall   remove Prism (keeps your settings and backups)
#   ./install.sh --purge       remove Prism and your settings and backups
#   ./install.sh --no-deps     skip installing system packages
#   ./install.sh --yes         don't ask before each step
#
# What it does, in order:
#   1. installs the system packages Prism needs to build, with your
#      distribution's package manager (asks for your password)
#   2. installs Rust if you don't have a recent enough version (from
#      rustup.rs, for your user only)
#   3. builds Prism (a few minutes the first time)
#   4. copies it to ~/.local/bin and adds it to your app menu
#   5. installs the device permission rules (asks for your password)
#
# Prism itself never runs as root. Nothing is sent anywhere except the
# downloads your package manager and Rust's installer make.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
bin_dir="${XDG_BIN_HOME:-$HOME/.local/bin}"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
config="${XDG_CONFIG_HOME:-$HOME/.config}"
rules=/etc/udev/rules.d/60-prism.rules
min_rust_minor=80   # Rust 1.80 or newer

mode=install
deps=yes
ask=yes
for a in "$@"; do
  case "$a" in
    --uninstall) mode=uninstall ;;
    --purge) mode=purge ;;
    --no-deps) deps=no ;;
    --yes|-y) ask=no ;;
    -h|--help) sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "Unknown option: $a (try --help)" >&2; exit 2 ;;
  esac
done

say() { printf '\n\033[1;35m==>\033[0m %s\n' "$*"; }
note() { printf '    %s\n' "$*"; }
fail() { printf '\n\033[1;31mStopped:\033[0m %s\n' "$*" >&2; exit 1; }
confirm() {
  [ "$ask" = no ] && return 0
  printf '    %s [Y/n] ' "$1"
  read -r reply </dev/tty 2>/dev/null || { echo "(no answer possible here, so: no)"; reply=n; }
  case "$reply" in ''|y|Y|yes|Yes) return 0 ;; *) return 1 ;; esac
}

[ "$(id -u)" -eq 0 ] && fail "Please run this as your normal user, not with sudo. It asks for your password when it needs it."

# ---------- uninstall ----------
if [ "$mode" != install ]; then
  say "Removing Prism"
  pkill -x prism-rgb 2>/dev/null && note "Closed the running Prism." || true
  rm -f "$bin_dir/prism-rgb" "$data/applications/prism-rgb.desktop" "$config/autostart/prism-rgb.desktop"
  for s in 32 64 128 256 512; do rm -f "$data/icons/hicolor/${s}x${s}/apps/prism-rgb.png"; done
  rm -f "$data/icons/hicolor/scalable/apps/prism-rgb.svg"
  note "Removed the app, its menu entry, icons and login start."
  if [ -f "$rules" ] && confirm "Remove the device permission rules too (needs your password)?"; then
    sudo rm -f "$rules" && sudo udevadm control --reload && note "Removed $rules."
  fi
  if [ "$mode" = purge ]; then
    rm -rf "$config/io.github.honeybadger.prism"
    note "Removed your Prism settings and onboard-memory backups."
  else
    note "Your settings and backups are kept in $config/io.github.honeybadger.prism"
    note "(remove them with: ./install.sh --purge)"
  fi
  say "Done. Your devices go back to their own lighting."
  exit 0
fi

# ---------- 1. system packages ----------
. /etc/os-release 2>/dev/null || true
family=""
for id in ${ID:-} ${ID_LIKE:-}; do
  case "$id" in
    arch|manjaro|endeavouros|garuda|cachyos) family=arch; break ;;
    debian|ubuntu|linuxmint|pop|elementary|zorin|neon) family=debian; break ;;
    fedora|rhel|centos|nobara|ultramarine) family=fedora; break ;;
    opensuse*|suse|sles) family=suse; break ;;
  esac
done

say "Prism installer for ${PRETTY_NAME:-this system}"
if [ "$deps" = yes ]; then
  case "$family" in
    arch)
      pkgs="base-devel webkit2gtk-4.1 hidapi libappindicator librsvg openssl pkgconf curl"
      cmd="sudo pacman -S --needed $pkgs" ;;
    debian)
      pkgs="build-essential curl wget file pkg-config libssl-dev libgtk-3-dev libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libhidapi-dev libudev-dev libxdo-dev"
      cmd="sudo apt-get install -y $pkgs" ;;
    fedora)
      pkgs="gcc gcc-c++ make curl wget file pkgconf-pkg-config openssl-devel webkit2gtk4.1-devel libappindicator-gtk3-devel librsvg2-devel hidapi-devel systemd-devel libxdo-devel"
      cmd="sudo dnf install -y $pkgs" ;;
    suse)
      pkgs="gcc gcc-c++ make curl wget file pkg-config libopenssl-devel webkit2gtk3-devel libappindicator3-devel librsvg-devel libhidapi-devel systemd-devel"
      cmd="sudo zypper install -y $pkgs" ;;
    *)
      note "I don't know this distribution's package names. Please install the"
      note "equivalents of: a C compiler, pkg-config, OpenSSL, WebKitGTK 4.1,"
      note "libappindicator (or ayatana), librsvg and hidapi, then run:"
      note "  ./install.sh --no-deps"
      exit 1 ;;
  esac
  say "Step 1 of 5: system packages"
  note "Prism needs these to build: $pkgs"
  note "It will run: $cmd"
  if confirm "Install them now (needs your password)?"; then
    [ "$family" = debian ] && sudo apt-get update
    $cmd
  else
    note "Skipped. If the build fails, install them and run this again."
  fi
fi

# ---------- 2. Rust ----------
say "Step 2 of 5: Rust"
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
rust_ok=no
if command -v cargo >/dev/null 2>&1; then
  minor=$(rustc --version 2>/dev/null | sed -n 's/^rustc 1\.\([0-9]*\).*/\1/p')
  if [ -n "$minor" ] && [ "$minor" -ge "$min_rust_minor" ]; then
    rust_ok=yes
    note "Found $(rustc --version)."
  else
    note "Your Rust ($(rustc --version 2>/dev/null)) is older than 1.$min_rust_minor."
  fi
fi
if [ "$rust_ok" = no ]; then
  note "Prism is written in Rust. The official installer (rustup.rs) installs it"
  note "for your user only, in ~/.rustup and ~/.cargo."
  confirm "Install Rust with rustup now?" || fail "Prism needs Rust 1.$min_rust_minor or newer to build."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
  . "$HOME/.cargo/env"
fi

# ---------- 3. build ----------
say "Step 3 of 5: building Prism (a few minutes the first time)"
cd "$here"
cargo build --release -p prism-rgb
[ -x target/release/prism-rgb ] || fail "The build didn't produce Prism. The messages above say why."

# ---------- 4. install for this user ----------
say "Step 4 of 5: installing for $(id -un)"
running=no
pgrep -x prism-rgb >/dev/null 2>&1 && running=yes && pkill -x prism-rgb && sleep 1
install -Dm755 target/release/prism-rgb "$bin_dir/prism-rgb"
src=src-tauri/icons
for s in 32 64 128; do [ -f "$src/${s}x${s}.png" ] && install -Dm644 "$src/${s}x${s}.png" "$data/icons/hicolor/${s}x${s}/apps/prism-rgb.png"; done
[ -f "$src/128x128@2x.png" ] && install -Dm644 "$src/128x128@2x.png" "$data/icons/hicolor/256x256/apps/prism-rgb.png"
[ -f "$src/icon.png" ] && install -Dm644 "$src/icon.png" "$data/icons/hicolor/512x512/apps/prism-rgb.png"
mkdir -p "$data/applications"
cat > "$data/applications/prism-rgb.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Prism
GenericName=RGB and key bindings
Comment=Corsair lighting, key bindings and onboard memory, plus GPU and motherboard lighting
Exec="$bin_dir/prism-rgb"
Icon=prism-rgb
Terminal=false
Categories=Utility;Settings;HardwareSettings;
Keywords=corsair;rgb;lighting;keyboard;mouse;icue;
StartupWMClass=prism-rgb
EOF
# Login start, if it was set up from an older location, now points here.
auto="$config/autostart/prism-rgb.desktop"
[ -f "$auto" ] && sed -i "s|^Exec=.*|Exec=\"$bin_dir/prism-rgb\" --hidden|" "$auto"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$data/applications" >/dev/null 2>&1 || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q "$data/icons/hicolor" 2>/dev/null || true
note "Installed $bin_dir/prism-rgb and added Prism to your app menu."
case ":$PATH:" in *":$bin_dir:"*) ;; *) note "($bin_dir isn't on your PATH; the app menu entry works anyway.)" ;; esac

# ---------- 5. device permissions ----------
say "Step 5 of 5: device permissions"
if [ -f "$rules" ] && cmp -s packaging/60-prism.rules "$rules"; then
  note "Already installed."
else
  note "So Prism can use your devices without root, this installs a rule giving"
  note "the person logged in at the computer access to: Corsair USB devices, the"
  note "ASUS Aura controller, Corsair key and button events (for bindings),"
  note "the virtual keyboard bindings type through, and only the one NVIDIA"
  note "bus the graphics card's lighting is on. See packaging/60-prism.rules."
  if confirm "Install the rule now (needs your password)?"; then
    sudo install -Dm644 packaging/60-prism.rules "$rules"
    sudo udevadm control --reload
    sudo udevadm trigger
    note "Installed. If a device isn't found, unplug and replug it once."
  else
    note "Skipped. Prism will only see devices you already have access to."
  fi
fi

say "Prism is installed. Open it from your app menu, or run: prism-rgb"
note "Update later by running this script again; remove with ./install.sh --uninstall"
if [ "$running" = yes ]; then
  (setsid "$bin_dir/prism-rgb" --hidden >/dev/null 2>&1 &) && note "Restarted Prism in the background."
fi
