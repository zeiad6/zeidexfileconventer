#!/usr/bin/env bash
# Bundles the redistributable engines into app/src-tauri/engines for the current Unix platform.
#
# The archive URLs and SHA-256 digests come from packaging/engines.json, so a version bump happens
# in one place.
#
# Every desktop package carries FFmpeg, FFprobe, 7-Zip, rhwp, Pandoc, qpdf and Ghostscript.
# qpdf handles PDF merge/split/pages/protection; Ghostscript renders PDF pages and compresses PDFs.
# The two are the difference between "the tool is listed" and "the tool runs" on a clean machine,
# so they are no longer left to the distribution. LibreOffice, Calibre, Tesseract and the AI models
# stay optional: bundling them would add several hundred megabytes for features not every user opens,
# and the Engines panel prints the one-line install command for each.
#
# Linux:  FFmpeg + FFprobe, 7-Zip, rhwp, Pandoc, qpdf, Ghostscript
# macOS:  the same (FFmpeg is bundled here too; Homebrew is no longer assumed)
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
  linux-*) components="ffmpeg 7zip rhwp pandoc qpdf" ;;
  # macOS gets single-binary FFmpeg/FFprobe builds (eugeneware/ffmpeg-static), so they are
  # downloaded raw rather than unpacked from an archive.
  osx-*) components="ffmpeg ffprobe 7zip rhwp pandoc qpdf" ;;
  *)
    echo "unsupported engine platform: $plat" >&2
    exit 1
    ;;
esac

# A local run can fetch a subset (useful when only one engine changed):
#   ZEIDEX_ENGINE_COMPONENTS="qpdf" bash packaging/fetch-engines.sh
# Ghostscript is part of the list too; ask for it by name when narrowing the set.
components="${ZEIDEX_ENGINE_COMPONENTS:-$components ghostscript}"

# Ghostscript, built by conda-forge. The Linux and macOS `gs` binaries are static, so only the
# executable and its Resource tree travel; no shared-object closure to maintain. macOS stays on
# 10.03.1: every newer conda build requires macOS 14.5, and the app still supports macOS 11.
MM_VERSION=2.10.0-0

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

  verify "$comp" "$archive" "$sha"

  mkdir -p "$tmp/$comp"
  case "$kind" in
    tar.xz) tar -xf "$archive" -C "$tmp/$comp" ;;
    tar.gz) tar -xf "$archive" -C "$tmp/$comp" ;;
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

field_kind() { field "$1" kind; }

fetch_raw() { # component, destination name
  local comp="$1" name="$2" url sha kind archive
  url="$(field "$comp" url)"
  sha="$(field "$comp" sha256 | tr 'A-F' 'a-f')"
  kind="$(field_kind "$comp")"
  archive="$tmp/$comp.raw"
  echo "==> $comp: $url"
  curl -fsSL "$url" -o "$archive"
  verify "$comp" "$archive" "$sha"
  case "$kind" in
    raw) cp "$archive" "$dst/$name" ;;
    # The gzipped macOS assets are half the size of the binaries they hold.
    raw.gz) gzip -dc "$archive" > "$dst/$name" ;;
    *)
      echo "unsupported raw kind '$kind' for $comp" >&2
      exit 1
      ;;
  esac
  chmod +x "$dst/$name"
}

fetch_tree() { # component, destination, subdirectories...
  local comp="$1" dest="$2"
  shift 2
  local url sha archive sub
  url="$(field "$comp" url)"
  sha="$(field "$comp" sha256 | tr 'A-F' 'a-f')"
  archive="$tmp/$comp.archive"
  echo "==> $comp: $url"
  curl -fsSL "$url" -o "$archive"
  verify "$comp" "$archive" "$sha"
  rm -rf "$tmp/$comp"
  mkdir -p "$tmp/$comp" "$dst/$dest"
  unzip -q "$archive" -d "$tmp/$comp"
  for sub in "$@"; do
    rm -rf "${dst:?}/$dest/$sub"
    # qpdf's archives put bin/ and lib/ at the top level; other trees wrap them in a folder.
    if [ -d "$tmp/$comp/$sub" ]; then
      cp -R "$tmp/$comp/$sub" "$dst/$dest/"
    else
      cp -R "$tmp/$comp"/*/"$sub" "$dst/$dest/"
    fi
  done
}

add_ghostscript() {
  local mm conda_plat sha url gs_version
  case "$plat" in
    linux-x64) mm=micromamba-linux-64; conda_plat=linux-64; gs_version=10.08.0 ;;
    linux-arm64) mm=micromamba-linux-aarch64; conda_plat=linux-aarch64; gs_version=10.08.0 ;;
    osx-x64) mm=micromamba-osx-64; conda_plat=osx-64; gs_version=10.03.1 ;;
    osx-arm64) mm=micromamba-osx-arm64; conda_plat=osx-arm64; gs_version=10.03.1 ;;
  esac
  # Digests are the ones published next to each micromamba release asset.
  case "$mm" in
    micromamba-linux-64) sha=701f39c06f8e6a670a0e186c8479b9e5c0502a2f0cda6cef65d83bdd2875e8bd ;;
    micromamba-linux-aarch64) sha=3bb56786e21b9b24024f599fe9185b4a59dc3f9f59937a42311ee583adbbb23c ;;
    micromamba-osx-64) sha=8421462a1ef1df9f7acd114f9e2a24c1332ce4f4e29f44465b4acb44e2755172 ;;
    micromamba-osx-arm64) sha=c547639f5caf742236641c362746abb4a3505774753ecd08fe341643670c4c01 ;;
  esac
  url="https://github.com/mamba-org/micromamba-releases/releases/download/${MM_VERSION}/${mm}"
  echo "==> ghostscript $gs_version via $mm"
  curl -fsSL "$url" -o "$tmp/$mm"
  verify "$mm" "$tmp/$mm" "$sha"
  chmod +x "$tmp/$mm"
  rm -rf "$tmp/gs"
  MAMBA_ROOT_PREFIX="$tmp/mamba" "$tmp/$mm" create -y -q -p "$tmp/gs" \
    -c conda-forge --platform "$conda_plat" "ghostscript==$gs_version"
  rm -rf "${dst:?}/gs"
  mkdir -p "$dst/gs/bin" "$dst/gs/share"
  cp "$tmp/gs/bin/gs" "$dst/gs/bin/gs"
  cp -R "$tmp/gs/share/ghostscript" "$dst/gs/share/"
  chmod +x "$dst/gs/bin/gs"
}

verify() { # label, archive, expected sha
  local actual
  if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$2" | awk '{print $1}')"
  else
    actual="$(shasum -a 256 "$2" | awk '{print $1}')"
  fi
  if [ "$actual" != "$3" ]; then
    echo "checksum mismatch for $1" >&2
    echo "  expected $3" >&2
    echo "  actual   $actual" >&2
    exit 1
  fi
}

for component in $components; do
  case "$component" in
    ffmpeg)
      case "$(field_kind ffmpeg)" in
        raw | raw.gz) fetch_raw ffmpeg ffmpeg ;;
        *) fetch ffmpeg ffmpeg ffprobe ;;
      esac
      ;;
    ffprobe) fetch_raw ffprobe ffprobe ;;
    7zip) fetch 7zip 7zz ;;
    rhwp) fetch rhwp rhwp ;;
    pandoc) fetch pandoc pandoc ;;
    qpdf) fetch_tree qpdf qpdf bin lib ;;
    ghostscript) add_ghostscript ;;
  esac
done

ls -la "$dst"
