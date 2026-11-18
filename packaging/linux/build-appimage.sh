#!/usr/bin/env bash
# Repackages Tauri's generated AppImage / AppDir using modern `appimagetool` and the static
# Type-2 runtime (AppImage/type2-runtime). This removes the legacy `libfuse2` dependency so the
# resulting `.AppImage` launches out-of-the-box on Ubuntu 22.04+, Fedora 36+, Debian 12+, etc.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
bundle_dir="$root/app/src-tauri/target/release/bundle/appimage"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

case "$(uname -m)" in
  x86_64 | amd64)
    ai_arch="x86_64"
    ;;
  aarch64 | arm64)
    ai_arch="aarch64"
    ;;
  *)
    echo "unsupported architecture for AppImage repack: $(uname -m)" >&2
    exit 1
    ;;
esac

appdir="$(find "$bundle_dir" -maxdepth 1 -type d -name '*.AppDir' | head -n 1 || true)"
appimage="$(find "$bundle_dir" -maxdepth 1 -type f -name '*.AppImage' | head -n 1 || true)"

if [ -z "$appdir" ] && [ -z "$appimage" ]; then
  echo "no AppDir or AppImage found under $bundle_dir" >&2
  exit 1
fi

if [ -z "$appdir" ]; then
  echo "==> extracting $appimage for Type-2 runtime repack"
  (
    cd "$tmp"
    chmod +x "$appimage"
    "$appimage" --appimage-extract >/dev/null
  )
  appdir="$tmp/squashfs-root"
fi

runtime="$tmp/runtime-$ai_arch"
tool="$tmp/appimagetool-$ai_arch.AppImage"

echo "==> fetching static Type-2 AppImage runtime ($ai_arch)"
curl -fsSL "https://github.com/AppImage/type2-runtime/releases/download/continuous/runtime-${ai_arch}" -o "$runtime"
chmod +x "$runtime"

if command -v appimagetool >/dev/null 2>&1; then
  tool_cmd="appimagetool"
else
  echo "==> fetching modern appimagetool ($ai_arch)"
  curl -fsSL "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-${ai_arch}.AppImage" -o "$tool"
  chmod +x "$tool"
  tool_cmd="$tool"
fi

if [ -z "$appimage" ]; then
  appimage="$bundle_dir/Zeidex.Convert_${ai_arch}.AppImage"
fi

out_tmp="$tmp/repacked.AppImage"
echo "==> building Type-2 AppImage: $(basename "$appimage")"
ARCH="$ai_arch" APPIMAGE_EXTRACT_AND_RUN=1 "$tool_cmd" --runtime-file "$runtime" --comp zstd "$appdir" "$out_tmp"
mv -f "$out_tmp" "$appimage"
chmod +x "$appimage"
echo "==> Type-2 AppImage ready: $appimage"
