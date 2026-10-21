#!/usr/bin/env bash
set -euo pipefail

# Prepares files and builds the standalone Flatpak bundle
APP_ID="com.zeidexsoft.convert"
BUILD_DIR="${BUILD_DIR:-.flatpak-build}"
REPO_DIR="${REPO_DIR:-.flatpak-repo}"
BUNDLE_OUT="${BUNDLE_OUT:-ZeidexConvert.flatpak}"

STAGE_DIR="packaging/flatpak/stage"
rm -rf "${STAGE_DIR}"
mkdir -p "${STAGE_DIR}/engines"

# Copy binary
if [ -f "app/src-tauri/target/release/zeidex-app" ]; then
  cp "app/src-tauri/target/release/zeidex-app" "${STAGE_DIR}/zeidex-app"
elif [ -f "target/release/zeidex-app" ]; then
  cp "target/release/zeidex-app" "${STAGE_DIR}/zeidex-app"
else
  echo "Error: zeidex-app binary not found in release target directories" >&2
  exit 1
fi

# Copy metadata and assets
cp packaging/flatpak/com.zeidexsoft.convert.desktop "${STAGE_DIR}/"
cp packaging/flatpak/com.zeidexsoft.convert.metainfo.xml "${STAGE_DIR}/"
cp app/src-tauri/icons/icon.png "${STAGE_DIR}/"

if [ -d "app/src-tauri/engines" ]; then
  cp -r app/src-tauri/engines/* "${STAGE_DIR}/engines/" 2>/dev/null || true
fi

cp packaging/flatpak/com.zeidexsoft.convert.yml "${STAGE_DIR}/manifest.yml"

echo "==> Building Flatpak in ${STAGE_DIR}..."
flatpak-builder --user --force-clean --repo="${REPO_DIR}" "${BUILD_DIR}" "${STAGE_DIR}/manifest.yml"
echo "==> Exporting single-file Flatpak bundle: ${BUNDLE_OUT}..."
flatpak build-bundle "${REPO_DIR}" "${BUNDLE_OUT}" "${APP_ID}"
echo "==> Success! Flatpak bundle created at: ${BUNDLE_OUT}"
