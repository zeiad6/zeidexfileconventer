#!/bin/sh
# Universal Linux installer for Zeidex Convert.
#
#   sh install.sh            install the desktop app (native package when the distro has one)
#   sh install.sh --cli      install only the zeidex-convert command line tool
#   sh install.sh --uninstall
#
# Picks the package the distro understands: .deb (apt), .rpm (dnf/zypper), the AUR recipe
# (pacman), otherwise the AppImage (any glibc distro) or the static musl CLI (Alpine and
# other musl systems, where the glibc GUI cannot run). Every download is checked against the
# release's SHA256SUMS file before anything is installed.
set -eu

VERSION="${ZEIDEX_VERSION:-0.2.0}"
REPO="https://github.com/zeiad6/zeidexfileconventer"
BASE="$REPO/releases/download/v$VERSION"
PREFIX="${PREFIX:-$HOME/.local}"
MODE="app"

for arg in "$@"; do
  case "$arg" in
    --cli) MODE="cli" ;;
    --uninstall) MODE="uninstall" ;;
    -h|--help) sed -n '2,11p' "$0"; exit 0 ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done

say() { printf '==> %s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

sudo_cmd() {
  if [ "$(id -u)" -eq 0 ]; then "$@"
  elif have sudo; then sudo "$@"
  elif have doas; then doas "$@"
  else die "root is needed for '$*'; rerun as root or install sudo"
  fi
}

fetch() { # url dest
  if have curl; then curl -fL --proto '=https' --tlsv1.2 -o "$2" "$1"
  elif have wget; then wget -O "$2" "$1"
  else die "curl or wget is required"
  fi
}

sha256_of() {
  if have sha256sum; then sha256sum "$1" | cut -d' ' -f1
  else shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

# GitHub turns spaces in asset names into dots, and the sums files list bundle paths
# ("deb/Zeidex Convert_0.2.0_amd64.deb"), so compare on that normalised basename.
verify() { # file asset sumsfile
  expected=$(awk -v want="$2" '{ line=$0; sub(/^[0-9a-f]+[ *]+/, "", line); n=split(line, p, "/");
             name=p[n]; gsub(/ /, ".", name); if (name == want) { print $1; exit } }' "$3")
  [ -n "$expected" ] || die "$2 is not listed in $(basename "$3")"
  actual=$(sha256_of "$1")
  [ "$expected" = "$actual" ] || die "checksum mismatch for $2 (expected $expected, got $actual)"
  say "verified $2"
}

download() { # asset sums -> prints local path
  fetch "$BASE/$2" "$TMP/$2" >&2
  fetch "$BASE/$1" "$TMP/$1" >&2
  verify "$TMP/$1" "$1" "$TMP/$2" >&2
  printf '%s' "$TMP/$1"
}

arch=$(uname -m)
case "$arch" in
  x86_64|amd64) arch=x86_64 ;;
  aarch64|arm64) arch=aarch64 ;;
  *) die "unsupported CPU: $arch" ;;
esac

is_musl() { ldd --version 2>&1 | grep -qi musl || [ -e /lib/ld-musl-"$arch".so.1 ]; }

install_cli() {
  if is_musl; then
    [ "$arch" = x86_64 ] || die "no musl build for $arch yet; build from source with cargo"
    target="$arch-unknown-linux-musl"
  else
    target="$arch-unknown-linux-gnu"
  fi
  name="zeidex-convert-$VERSION-$target"
  file=$(download "$name.tar.gz" "SHA256SUMS-cli-$name.txt")
  mkdir -p "$PREFIX/bin"
  tar -xzf "$file" -C "$TMP"
  install -m 0755 "$TMP/$name" "$PREFIX/bin/zeidex-convert"
  say "installed $PREFIX/bin/zeidex-convert"
  case ":$PATH:" in *":$PREFIX/bin:"*) ;; *) say "add $PREFIX/bin to PATH" ;; esac
}

install_appimage() {
  [ "$arch" = x86_64 ] || die "the AppImage is built for x86_64 only; use --cli on $arch"
  asset="Zeidex.Convert_${VERSION}_amd64.AppImage"
  file=$(download "$asset" SHA256SUMS-linux.txt)
  mkdir -p "$PREFIX/bin" "$PREFIX/share/applications"
  install -m 0755 "$file" "$PREFIX/bin/zeidex-convert-app"
  cat > "$PREFIX/share/applications/com.zeidexsoft.convert.desktop" <<EOF
[Desktop Entry]
Name=Zeidex Convert
Comment=Local file converter & media processor
Comment[ar]=محوّل ملفات ومعالج وسائط محلي
Exec=$PREFIX/bin/zeidex-convert-app %U
Icon=com.zeidexsoft.convert
Terminal=false
Type=Application
Categories=Utility;AudioVideo;Graphics;
StartupWMClass=zeidex-app
EOF
  have update-desktop-database && update-desktop-database "$PREFIX/share/applications" || true
  if ! ldconfig -p 2>/dev/null | grep -q libfuse.so.2; then
    say "libfuse2 is missing: install it, or run the AppImage with --appimage-extract-and-run"
  fi
  say "installed $PREFIX/bin/zeidex-convert-app"
}

install_app() {
  if [ "$arch" = x86_64 ] && have apt-get; then
    file=$(download "Zeidex.Convert_${VERSION}_amd64.deb" SHA256SUMS-linux.txt)
    sudo_cmd apt-get install -y "$file"
  elif [ "$arch" = x86_64 ] && { have dnf || have zypper; }; then
    file=$(download "Zeidex.Convert-${VERSION}-1.x86_64.rpm" SHA256SUMS-linux.txt)
    if have dnf; then sudo_cmd dnf install -y "$file"; else sudo_cmd zypper --non-interactive install --allow-unsigned-rpm "$file"; fi
  elif [ "$arch" = x86_64 ] && have pacman && have makepkg; then
    [ "$(id -u)" -ne 0 ] || die "makepkg refuses to run as root; rerun as a normal user"
    mkdir -p "$TMP/aur"
    fetch "https://raw.githubusercontent.com/zeiad6/zeidexfileconventer/v$VERSION/packaging/linux/arch/PKGBUILD" "$TMP/aur/PKGBUILD"
    (cd "$TMP/aur" && makepkg -si --noconfirm)
  elif is_musl; then
    say "musl system: the desktop app needs glibc, installing the CLI instead"
    install_cli
    return
  elif have flatpak && [ "$arch" = x86_64 ]; then
    fetch "$BASE/ZeidexConvert.flatpak" "$TMP/ZeidexConvert.flatpak"
    fetch "$BASE/SHA256SUMS-flatpak.txt" "$TMP/SHA256SUMS-flatpak.txt"
    verify "$TMP/ZeidexConvert.flatpak" ZeidexConvert.flatpak "$TMP/SHA256SUMS-flatpak.txt"
    flatpak install --user -y "$TMP/ZeidexConvert.flatpak"
  else
    install_appimage
  fi
  # "Open with Zeidex" in Nautilus, Nemo, Dolphin and Thunar; the CLI writes the entries.
  if have zeidex-convert || [ -x "$PREFIX/bin/zeidex-convert" ]; then
    PATH="$PREFIX/bin:$PATH" zeidex-convert integrate || say "file manager integration skipped"
  fi
}

uninstall() {
  if have dpkg && dpkg -s zeidex-convert >/dev/null 2>&1; then sudo_cmd apt-get remove -y zeidex-convert; fi
  if have rpm && rpm -q zeidex-convert >/dev/null 2>&1; then sudo_cmd rpm -e zeidex-convert; fi
  if have pacman && pacman -Q zeidex-convert-bin >/dev/null 2>&1; then sudo_cmd pacman -R --noconfirm zeidex-convert-bin; fi
  if have flatpak && flatpak info com.zeidexsoft.convert >/dev/null 2>&1; then flatpak uninstall --user -y com.zeidexsoft.convert; fi
  rm -f "$PREFIX/bin/zeidex-convert" "$PREFIX/bin/zeidex-convert-app" \
        "$PREFIX/share/applications/com.zeidexsoft.convert.desktop" \
        "$HOME/.local/share/nautilus/scripts/Open with Zeidex" \
        "$HOME/.local/share/nemo/actions/zeidex.nemo_action" \
        "$HOME/.local/share/kio/servicemenus/zeidex.desktop" \
        "$HOME/.local/share/Thunar/sendto/zeidex.desktop"
  say "removed"
}

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT INT TERM

case "$MODE" in
  cli) install_cli ;;
  app) install_app ;;
  uninstall) uninstall ;;
esac
