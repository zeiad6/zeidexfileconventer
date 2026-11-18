#!/usr/bin/env bash
# Bundles the redistributable engines into app/src-tauri/engines for the current Unix platform.
#
# The archive URLs and SHA-256 digests come from packaging/engines.json, so a version bump happens
# in one place.
#
# Every desktop package carries FFmpeg, FFprobe, 7-Zip, rhwp, Pandoc, qpdf and Ghostscript.
# qpdf handles PDF merge/split/pages/protection; Ghostscript renders PDF pages and compresses PDFs.
# The two are the difference between "the tool is listed" and "the tool runs" on a clean machine,
# so they are no longer left to the distribution.
#
# Also bundled: libvips, Tesseract (eng+ara+osd), qrencode, ExifTool, ZBar (macOS), whisper.cpp
# (Linux) and Real-ESRGAN where an archive exists, plus the whisper-tiny and u2net models.
# LibreOffice and Calibre stay optional on Unix: the distro package is one command and the
# Engines panel runs it, while their archives are 300-400 MB each and are not relocatable
# without an install. On Windows both are bundled, because that is where the app used to die
# with "LibreOffice not found".
#
# Linux:  FFmpeg + FFprobe, 7-Zip, rhwp, Pandoc, qpdf, Ghostscript, libvips, Tesseract, qrencode,
#         ExifTool, whisper.cpp, Real-ESRGAN (x64), models
# macOS:  the same, with ZBar and without whisper.cpp (upstream publishes no macOS binary)
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
dst="$root/app/src-tauri/engines"
json="$root/packaging/engines.json"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$dst"

profile="${ZEIDEX_BUILD_PROFILE:-full}"
while [ "$#" -gt 0 ]; do
  case "$1" in
    --profile)
      profile="${2:-}"
      shift 2
      ;;
    --profile=*)
      profile="${1#*=}"
      shift
      ;;
    *)
      echo "unknown option: $1 (expected --profile lite|full)" >&2
      exit 1
      ;;
  esac
done

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

case "$profile" in
  lite)
    case "$plat" in
      linux-*) components="ffmpeg 7zip vips-lite" ;;
      osx-*) components="ffmpeg ffprobe 7zip vips-lite" ;;
      *)
        echo "unsupported engine platform: $plat" >&2
        exit 1
        ;;
    esac
    components="${ZEIDEX_ENGINE_COMPONENTS:-$components}"
    ;;
  full)
    case "$plat" in
      # Linux also carries libvips, Tesseract and qrencode ("extras"); macOS and Windows do not yet.
      linux-*) components="ffmpeg 7zip rhwp pandoc qpdf extras" ;;
      # macOS gets single-binary FFmpeg/FFprobe builds (eugeneware/ffmpeg-static), so they are
      # downloaded raw rather than unpacked from an archive. The conda extras (libvips, Tesseract
      # with eng+ara, qrencode) travel here too: OCR and QR used to be Linux-only by accident.
      osx-*) components="ffmpeg ffprobe 7zip rhwp pandoc qpdf extras" ;;
      *)
        echo "unsupported engine platform: $plat" >&2
        exit 1
        ;;
    esac

    # A local run can fetch a subset (useful when only one engine changed):
    #   ZEIDEX_ENGINE_COMPONENTS="qpdf" bash packaging/fetch-engines.sh
    # Ghostscript is part of the list too; ask for it by name when narrowing the set.
    components="${ZEIDEX_ENGINE_COMPONENTS:-$components ghostscript exiftool models}"

    # Optional per platform: whisper.cpp has no macOS binary in its releases and Real-ESRGAN has no
    # Linux arm64 build, so each is listed only where a verified archive exists.
    case "$plat" in
      linux-x64) components="$components whisper realesrgan" ;;
      linux-arm64) components="$components whisper" ;;
      osx-*) components="$components zbar realesrgan" ;;
    esac
    ;;
  *)
    echo "unsupported profile '$profile' (expected lite or full)" >&2
    exit 1
    ;;
esac

echo "==> engine profile: $profile ($plat)"

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

# Leaves a verified micromamba at $tmp/$mm and sets $conda_plat.
get_micromamba() {
  local sha
  case "$plat" in
    linux-x64) mm=micromamba-linux-64; conda_plat=linux-64 ;;
    linux-arm64) mm=micromamba-linux-aarch64; conda_plat=linux-aarch64 ;;
    osx-x64) mm=micromamba-osx-64; conda_plat=osx-64 ;;
    osx-arm64) mm=micromamba-osx-arm64; conda_plat=osx-arm64 ;;
  esac
  [ -x "$tmp/$mm" ] && return
  # Digests are the ones published next to each micromamba release asset.
  case "$mm" in
    micromamba-linux-64) sha=701f39c06f8e6a670a0e186c8479b9e5c0502a2f0cda6cef65d83bdd2875e8bd ;;
    micromamba-linux-aarch64) sha=3bb56786e21b9b24024f599fe9185b4a59dc3f9f59937a42311ee583adbbb23c ;;
    micromamba-osx-64) sha=8421462a1ef1df9f7acd114f9e2a24c1332ce4f4e29f44465b4acb44e2755172 ;;
    micromamba-osx-arm64) sha=c547639f5caf742236641c362746abb4a3505774753ecd08fe341643670c4c01 ;;
  esac
  curl -fsSL "https://github.com/mamba-org/micromamba-releases/releases/download/${MM_VERSION}/${mm}" -o "$tmp/$mm"
  verify "$mm" "$tmp/$mm" "$sha"
  chmod +x "$tmp/$mm"
}

add_ghostscript() {
  local gs_version
  case "$plat" in
    linux-*) gs_version=10.08.0 ;;
    osx-*) gs_version=10.03.1 ;;
  esac
  get_micromamba
  echo "==> ghostscript $gs_version via $mm"
  rm -rf "$tmp/gs"
  MAMBA_ROOT_PREFIX="$tmp/mamba" "$tmp/$mm" create -y -q -p "$tmp/gs" \
    -c conda-forge --platform "$conda_plat" "ghostscript==$gs_version"
  rm -rf "${dst:?}/gs"
  mkdir -p "$dst/gs/bin" "$dst/gs/share"
  cp "$tmp/gs/bin/gs" "$dst/gs/bin/gs"
  cp -R "$tmp/gs/share/ghostscript" "$dst/gs/share/"
  chmod +x "$dst/gs/bin/gs"
}

# libvips, Tesseract (English, Arabic and orientation data) and qrencode, built by conda-forge.
# Tesseract's configs/ and pdf.ttf travel too: without them `tsv` and `pdf` output fail.
# The environment weighs over a gigabyte; slim-conda.py keeps the four programs, the HEIC/AVIF
# and JPEG XL modules and only the libraries they load. The runner points each program at its
# moved data (tools::bundled_env), so the tree must keep the bin/ lib/ share/ layout.
add_extras() {
  get_micromamba
  echo "==> libvips, tesseract, qrencode via $mm"
  rm -rf "$tmp/extras"
  MAMBA_ROOT_PREFIX="$tmp/mamba" "$tmp/$mm" create -y -q -p "$tmp/extras" \
    -c conda-forge --platform "$conda_plat" "libvips==8.18.7" "tesseract==5.5.4" "libqrencode==4.1.1"
  rm -rf "${dst:?}/extras"
  python3 "$root/packaging/slim-conda.py" "$tmp/extras" "$dst/extras" \
    vips vipsheader tesseract qrencode \
    lib/vips-modules-8.18/vips-heif.so lib/vips-modules-8.18/vips-jxl.so \
    share/tessdata/eng.traineddata share/tessdata/ara.traineddata share/tessdata/osd.traineddata \
    share/tessdata/configs share/tessdata/tessconfigs share/tessdata/pdf.ttf
  # Gate: a loader failure (a library the closure missed) has to stop the build here, not the
  # user's first OCR. The Windows script runs the same check for the same reason.
  TESSDATA_PREFIX="$dst/extras/share/tessdata" "$dst/extras/bin/tesseract" --version >/dev/null ||
    { echo "the staged tesseract does not start" >&2; exit 1; }
  TESSDATA_PREFIX="$dst/extras/share/tessdata" "$dst/extras/bin/tesseract" --tessdata-dir "$dst/extras/share/tessdata" --list-langs |
    grep -qx "ara" || { echo "staged tesseract is missing ara.traineddata" >&2; exit 1; }
}

# Lite mode: slim libvips only (no Tesseract or qrencode).
add_vips_lite() {
  get_micromamba
  echo "==> libvips (lite) via $mm"
  rm -rf "$tmp/vips-lite"
  MAMBA_ROOT_PREFIX="$tmp/mamba" "$tmp/$mm" create -y -q -p "$tmp/vips-lite" \
    -c conda-forge --platform "$conda_plat" "libvips==8.18.7"
  rm -rf "${dst:?}/extras"
  python3 "$root/packaging/slim-conda.py" "$tmp/vips-lite" "$dst/extras" \
    vips vipsheader \
    lib/vips-modules-8.18/vips-heif.so lib/vips-modules-8.18/vips-jxl.so
}

# ExifTool: its script adds `<its own dir>/lib` to @INC, so the official tarball's `lib/` is
# what makes it self-contained. A conda build ships only the wrapper and then quietly loads
# whatever Image::ExifTool the host happens to have — a different version, or none.
add_exiftool() {
  local url sha archive
  url="$(field exiftool url)"
  sha="$(field exiftool sha256 | tr 'A-F' 'a-f')"
  archive="$tmp/exiftool.tar.gz"
  echo "==> exiftool: $url"
  curl -fsSL "$url" -o "$archive"
  verify exiftool "$archive" "$sha"
  rm -rf "$tmp/exiftool" "${dst:?}/exiftool"
  mkdir -p "$tmp/exiftool" "$dst/exiftool/bin"
  tar -xf "$archive" -C "$tmp/exiftool" --strip-components=1
  cp "$tmp/exiftool/exiftool" "$dst/exiftool/bin/exiftool"
  cp -R "$tmp/exiftool/lib" "$dst/exiftool/bin/lib"
  chmod +x "$dst/exiftool/bin/exiftool"
}

# whisper.cpp: the CLI plus the shared ggml/whisper libraries it loads through $ORIGIN.
add_whisper() {
  local url sha archive sub
  url="$(field whisper url)"
  sha="$(field whisper sha256 | tr 'A-F' 'a-f')"
  archive="$tmp/whisper.archive"
  echo "==> whisper: $url"
  curl -fsSL "$url" -o "$archive"
  verify whisper "$archive" "$sha"
  rm -rf "$tmp/whisper" "${dst:?}/whisper"
  mkdir -p "$tmp/whisper" "$dst/whisper/bin"
  tar -xf "$archive" -C "$tmp/whisper"
  sub="$(find "$tmp/whisper" -type f -name whisper-cli | head -n 1)"
  [ -n "$sub" ] || { echo "whisper-cli was not found inside the archive" >&2; exit 1; }
  # The libraries sit beside the binary and the rpath is $ORIGIN, so the whole directory ships.
  cp -R "$(dirname "$sub")/." "$dst/whisper/bin/"
  chmod +x "$dst/whisper/bin/whisper-cli"
}

# Real-ESRGAN ncnn/Vulkan build: the executable and its models/ folder, which the runner reads
# from beside the engine (tools::esrgan_models).
add_realesrgan() {
  local url sha archive
  url="$(field realesrgan url)"
  sha="$(field realesrgan sha256 | tr 'A-F' 'a-f')"
  archive="$tmp/realesrgan.zip"
  echo "==> realesrgan: $url"
  curl -fsSL "$url" -o "$archive"
  verify realesrgan "$archive" "$sha"
  rm -rf "$tmp/realesrgan" "${dst:?}/realesrgan"
  mkdir -p "$tmp/realesrgan"
  unzip -q "$archive" -d "$tmp/realesrgan"
  # Strip the release's sample images; the app only needs the binary and the networks.
  find "$tmp/realesrgan" -maxdepth 2 -type f \( -name '*.jpg' -o -name '*.png' \) -delete
  cp -R "$tmp/realesrgan" "$dst/realesrgan"
  find "$dst/realesrgan" -type f -name 'realesrgan-ncnn-vulkan' -exec chmod +x {} +
}

# ZBar on macOS: the release binary looks for /usr/local/lib/libzbar.0.dylib by absolute path,
# which no package would have. Point it at the copy that travels beside it.
add_zbar() {
  local url sha archive
  url="$(field zbar url)"
  sha="$(field zbar sha256 | tr 'A-F' 'a-f')"
  archive="$tmp/zbar.tar.gz"
  echo "==> zbar: $url"
  curl -fsSL "$url" -o "$archive"
  verify zbar "$archive" "$sha"
  rm -rf "$tmp/zbar" "${dst:?}/zbar"
  mkdir -p "$tmp/zbar" "$dst/zbar/bin" "$dst/zbar/lib"
  tar -xf "$archive" -C "$tmp/zbar"
  cp "$tmp/zbar/usr/local/bin/zbarimg" "$dst/zbar/bin/zbarimg"
  cp "$tmp/zbar/usr/local/lib/libzbar.0.dylib" "$dst/zbar/lib/"
  chmod +x "$dst/zbar/bin/zbarimg"
  if command -v install_name_tool >/dev/null 2>&1; then
    install_name_tool -change /usr/local/lib/libzbar.0.dylib @loader_path/../lib/libzbar.0.dylib \
      "$dst/zbar/bin/zbarimg"
  else
    echo "install_name_tool is missing; zbarimg will not find its library" >&2
  fi
}

# The two model files that every platform shares: whisper's tiny network and rembg's U²-Net.
add_models() {
  local key name url sha archive
  for key in whisper-tiny u2net; do
    name="$(python3 - "$json" "$key" <<'PY'
import json, sys
print(json.load(open(sys.argv[1]))["models"][sys.argv[2]]["name"])
PY
)"
    url="$(python3 - "$json" "$key" <<'PY'
import json, sys
print(json.load(open(sys.argv[1]))["models"][sys.argv[2]]["url"])
PY
)"
    sha="$(python3 - "$json" "$key" <<'PY'
import json, sys
print(json.load(open(sys.argv[1]))["models"][sys.argv[2]]["sha256"].lower())
PY
)"
    archive="$tmp/$name"
    echo "==> model $name: $url"
    curl -fsSL "$url" -o "$archive"
    verify "$name" "$archive" "$sha"
    mkdir -p "$dst/models"
    cp "$archive" "$dst/models/$name"
  done
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
    qpdf)
      fetch_tree qpdf qpdf bin lib
      # qpdf's Linux archive links libgmp (through GnuTLS) but does not carry it, and a minimal
      # desktop may not have it. Taken from the build host, so only when fetching for the host.
      if [ "${plat%%-*}" = linux ] && [ -z "${ZEIDEX_ENGINE_PLATFORM:-}" ]; then
        gmp="$(PATH="$PATH:/sbin:/usr/sbin" ldconfig -p | awk '/libgmp\.so\.10 /{p=$NF} END{print p}')"
        [ -n "$gmp" ] || { echo "libgmp.so.10 was not found on this host" >&2; exit 1; }
        cp -L "$gmp" "$dst/qpdf/lib/libgmp.so.10"
      fi
      ;;
    ghostscript) add_ghostscript ;;
    extras) add_extras ;;
    vips-lite) add_vips_lite ;;
    exiftool) add_exiftool ;;
    whisper) add_whisper ;;
    realesrgan) add_realesrgan ;;
    zbar) add_zbar ;;
    models) add_models ;;
  esac
done

ls -la "$dst"
