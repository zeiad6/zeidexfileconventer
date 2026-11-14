#!/bin/sh
# Per-user installer for Zeidex Convert on Linux.
#
#   sh install.sh              install the AppImage into ~/.local (no sudo, no system package)
#   sh install.sh --uninstall  remove the launcher, icon and desktop entry
#
# One package covers every distribution: the AppImage is built on Ubuntu 22.04, so it needs
# glibc 2.35 or newer (Debian 12+, Ubuntu 22.04+, Fedora 36+, Mint 21+, openSUSE 15.5+).
# Nothing is written outside the user's home, so no administrator prompt ever appears.
set -eu

VERSION="${ZEIDEX_VERSION:-0.3.1}"
REPO="https://github.com/zeiad6/zeidexfileconventer"
BASE="$REPO/releases/download/v$VERSION"
ASSET=""
SUMS=""

PREFIX="${PREFIX:-$HOME/.local}"
BIN="$PREFIX/bin"
APPS="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
ICONS="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/256x256/apps"
TARGET="$BIN/zeidex-convert.AppImage"
LINK="$BIN/zeidex-convert"
DESKTOP="$APPS/zeidex-convert.desktop"
ICON="$ICONS/zeidex-convert.png"
TMP="${TMPDIR:-/tmp}/zeidex-install.$$"
trap 'rm -rf "$TMP"' EXIT INT TERM

say() { printf '%s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

have() { command -v "$1" >/dev/null 2>&1; }

fetch() { # url dest
  if have curl; then
    curl -fsSL --retry 3 --connect-timeout 20 "$1" -o "$2"
  elif have wget; then
    wget -q -O "$2" "$1"
  else
    die "curl or wget is required"
  fi
}

uninstall() {
  rm -f "$DESKTOP" "$ICON" "$LINK" "$TARGET"
  say "removed Zeidex Convert from $PREFIX"
  exit 0
}

[ "${1:-}" = "--uninstall" ] && uninstall
[ "${1:-}" = "--help" ] && { sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'; exit 0; }

case "$(uname -m)" in
  x86_64 | amd64) arch_tag=amd64 ;;
  aarch64 | arm64) arch_tag=aarch64 ;;
  *) die "unsupported CPU: $(uname -m); build from source: https://github.com/zeiad6/zeidexfileconventer#build" ;;
esac
ASSET="Zeidex.Convert_${VERSION}_${arch_tag}.AppImage"
SUMS="SHA256SUMS-linux-${arch_tag}.txt"

command -v sha256sum >/dev/null 2>&1 || die "sha256sum is required"

mkdir -p "$BIN" "$APPS" "$ICONS" "$TMP"
say "==> downloading Zeidex Convert $VERSION"
fetch "$BASE/$SUMS" "$TMP/$SUMS"
fetch "$BASE/$ASSET" "$TMP/$ASSET"

expected="$(awk -v want="$ASSET" '{ name=$2; sub(/^\.\//, "", name); if (name == want) { print $1; exit } }' "$TMP/$SUMS")"
[ -n "$expected" ] || die "$ASSET is not listed in $SUMS"
actual="$(sha256sum "$TMP/$ASSET" | cut -d' ' -f1)"
[ "$expected" = "$actual" ] || die "checksum mismatch for $ASSET"
say "==> verified SHA-256"

install -m 755 "$TMP/$ASSET" "$TARGET.new"
mv -f "$TARGET.new" "$TARGET"
ln -sf "zeidex-convert.AppImage" "$LINK"

# The icon ships inside the bundle; extract it without running the app.
if [ -x "$TARGET" ]; then
  "$TARGET" --appimage-extract 'zeidex-convert.png' >/dev/null 2>&1 || \
    "$TARGET" --appimage-extract '*.png' >/dev/null 2>&1 || true
  found="$(find squashfs-root -type f -path '*icons*' -name '*.png' 2>/dev/null | head -n 1)"
  [ -n "$found" ] && cp "$found" "$ICON"
  rm -rf squashfs-root
fi

cat > "$DESKTOP" <<EOF
[Desktop Entry]
Type=Application
Version=1.0
Name=Zeidex Convert
Name[ar]=Zeidex Convert
Comment=Offline file converter and media processor
Comment[ar]=محوّل ومعالج ملفات ووسائط يعمل دون إنترنت
Exec=$TARGET %F
TryExec=$TARGET
Icon=zeidex-convert
Terminal=false
Categories=Utility;Graphics;AudioVideo;
MimeType=image/jpeg;image/png;image/webp;image/avif;image/heic;image/tiff;video/mp4;video/x-matroska;audio/mpeg;application/pdf;
EOF

if have update-desktop-database; then update-desktop-database "$APPS" >/dev/null 2>&1 || true; fi
if have gtk-update-icon-cache; then gtk-update-icon-cache -q "${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor" >/dev/null 2>&1 || true; fi

say "==> installed to $TARGET"
say "    run: zeidex-convert   (or open it from your application menu)"
