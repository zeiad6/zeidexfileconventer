#!/usr/bin/env bash
# Bundles portable FFmpeg and 7-Zip into the Linux build so the AppImage works out of the box.
# libvips, Ghostscript and qpdf come from the distro (declared as .deb dependencies);
# LibreOffice, Pandoc, Calibre and Tesseract are optional and listed in the Engines panel.
set -euo pipefail

dst="$(cd "$(dirname "$0")/.." && pwd)/app/src-tauri/engines"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$dst"
auth=()
[[ -n "${GITHUB_TOKEN:-}" ]] && auth=(-H "Authorization: Bearer $GITHUB_TOKEN")

asset_url() { # repo, regex
  curl -fsSL "${auth[@]}" -H "User-Agent: zeidex-ci" "https://api.github.com/repos/$1/releases/latest" |
    python3 -c "import sys,json,re;print(next(a['browser_download_url'] for a in json.load(sys.stdin)['assets'] if re.match(r'$2',a['name'])))"
}

curl -fsSL "$(asset_url BtbN/FFmpeg-Builds '^ffmpeg-master-latest-linux64-gpl\.tar\.xz$')" -o "$tmp/ffmpeg.tar.xz"
tar -xJf "$tmp/ffmpeg.tar.xz" -C "$tmp"
cp "$tmp"/ffmpeg-*/bin/ffmpeg "$tmp"/ffmpeg-*/bin/ffprobe "$dst/"

curl -fsSL "$(asset_url ip7z/7zip '^7z[0-9]+-linux-x64\.tar\.xz$')" -o "$tmp/7z.tar.xz"
mkdir -p "$tmp/7z" && tar -xJf "$tmp/7z.tar.xz" -C "$tmp/7z"
cp "$tmp/7z/7zz" "$dst/"

chmod +x "$dst"/*
ls -la "$dst"
