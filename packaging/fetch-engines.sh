#!/usr/bin/env bash
# Bundles the redistributable engines into app/src-tauri/engines for the current Unix platform.
#
# The archive URLs and SHA-256 digests come from packaging/engines.json, so a version bump happens
# in one place. Engines whose licences or sizes make bundling a bad fit (Ghostscript AGPL,
# LibreOffice, Calibre, Tesseract) are not downloaded: the app's Engines panel prints the install
# command for each missing tool, and the Linux .deb/.rpm declare the distro packages.
#
# Linux:  FFmpeg + FFprobe, 7-Zip, rhwp, Pandoc
# macOS:  7-Zip, rhwp, Pandoc (FFmpeg comes from Homebrew; see the note above)
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
dst="$root/app/src-tauri/engines"
json="$root/packaging/engines.json"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$dst"

if [ -n "${ZEIDEX_ENGINE_PLATFORM:-}" ]; then
  plat="$ZEIDEX_ENGINE_PLATFORM"
else
  case "$(uname -s):$(uname -m)" in
    Linux:x86_64) plat=linux-x64 ;;
    Linux:aarch64 | Linux:arm64) plat=linux-arm64 ;;
    Darwin:arm64) plat=osx-arm64 ;;
    Darwin:x86_64) plat=osx-x64 ;;
    *)
      echo "unsupported platform: $(uname -s) $(uname -m)" >&2
      exit 1
      ;;
  esac
fi

case "$plat" in
  linux-*) components="ffmpeg 7zip rhwp pandoc" ;;
  osx-*) components="7zip rhwp pandoc" ;;
  *)
    echo "unsupported engine platform: $plat" >&2
    exit 1
    ;;
esac

field() { # component, key
  python3 - "$json" "$plat" "$1" "$2" <<'PY'
import json, sys
doc = json.load(open(sys.argv[1]))
entry = doc["platforms"][sys.argv[2]]["components"][sys.argv[3]]
print(entry[sys.argv[4]])
PY
}

fetch() { # component, binary names...
  local comp="$1"
  shift
  local url sha kind archive found name
  url="$(field "$comp" url)"
  sha="$(field "$comp" sha256 | tr 'A-F' 'a-f')"
  kind="$(field "$comp" kind)"
  archive="$tmp/$comp.archive"

  echo "==> $comp: $url"
  curl -fsSL "$url" -o "$archive"

  local actual
  if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$archive" | awk '{print $1}')"
  else
    actual="$(shasum -a 256 "$archive" | awk '{print $1}')"
  fi
  if [ "$actual" != "$sha" ]; then
    echo "checksum mismatch for $comp" >&2
    echo "  expected $sha" >&2
    echo "  actual   $actual" >&2
    exit 1
  fi

  mkdir -p "$tmp/$comp"
  case "$kind" in
    tar.xz) tar -xJf "$archive" -C "$tmp/$comp" ;;
    tar.gz) tar -xzf "$archive" -C "$tmp/$comp" ;;
    zip) unzip -q "$archive" -d "$tmp/$comp" ;;
    *)
      echo "unsupported archive kind '$kind' for $comp" >&2
      exit 1
      ;;
  esac

  for name in "$@"; do
    found="$(find "$tmp/$comp" -type f -name "$name" | head -n 1)"
    if [ -z "$found" ]; then
      echo "$name was not found inside $comp" >&2
      exit 1
    fi
    cp "$found" "$dst/$name"
    chmod +x "$dst/$name"
  done
}

for component in $components; do
  case "$component" in
    ffmpeg) fetch ffmpeg ffmpeg ffprobe ;;
    7zip) fetch 7zip 7zz ;;
    rhwp) fetch rhwp rhwp ;;
    pandoc) fetch pandoc pandoc ;;
  esac
done

ls -la "$dst"
