#!/usr/bin/env bash
# Real-engine smoke test: builds fixtures with FFmpeg/Ghostscript, runs every major route
# through the CLI, and checks each output exists and is non-empty. Routes whose engine is
# missing are skipped, so it also runs on a partial install.
# Usage: packaging/smoke.sh path/to/zeidex
set -uo pipefail

Z="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
cd "$work"
pass=0 fail=0 skip=0

has() { command -v "$1" >/dev/null 2>&1; }
# check <expected output> <needed binaries...> -- <zeidex args...>
check() {
  local out="$1"; shift
  local needs=()
  while [[ "$1" != "--" ]]; do needs+=("$1"); shift; done
  shift
  for n in "${needs[@]}"; do
    if ! has "$n"; then echo "SKIP  $* (no $n)"; skip=$((skip + 1)); return; fi
  done
  if "$Z" "$@" >/dev/null 2>err.log && [[ -s "$out" || -d "$out" ]]; then
    echo "ok    $*"; pass=$((pass + 1))
  else
    echo "FAIL  $*"; sed 's/^/      /' err.log; fail=$((fail + 1))
  fi
}

ffmpeg -loglevel error -f lavfi -i testsrc=size=640x360:rate=25 -f lavfi -i sine=frequency=440 -t 6 \
  -c:v libx264 -pix_fmt yuv420p -c:a aac clip.mp4
ffmpeg -loglevel error -f lavfi -i sine=frequency=300:duration=5 voice.wav
ffmpeg -loglevel error -f lavfi -i testsrc=size=800x600 -frames:v 1 photo.png
ffmpeg -loglevel error -f lavfi -i testsrc2=size=400x300 -frames:v 1 shot.bmp
printf '%%!PS\n/Helvetica findfont 48 scalefont setfont\n72 600 moveto (Zeidex OCR test) show\nshowpage\n72 600 moveto (Page two) show\nshowpage\n' > doc.ps
gs -q -dSAFER -dBATCH -dNOPAUSE -sDEVICE=pdfwrite -sOutputFile=doc.pdf doc.ps
cp doc.pdf other.pdf
printf '1\n00:00:01,000 --> 00:00:02,500\nHello\n' > subs.srt
printf '# Title\n\nSome *markdown* text.\n' > notes.md
mkdir folder && echo hi > folder/a.txt

check clip.webm             ffmpeg -- convert --to webm clip.mp4
check clip.gif              ffmpeg -- convert --to gif clip.mp4
check voice.flac            ffmpeg -- convert --to flac voice.wav
check clip-compressed.mp4   ffmpeg ffprobe -- tool compress --target 200KB clip.mp4
check clip-trimmed.mp4      ffmpeg -- tool trim --start 1 --end 3 clip.mp4
check clip-2x.mp4           ffmpeg ffprobe -- tool speed --factor 2 clip.mp4
check "clip parts"          ffmpeg ffprobe -- tool split --parts 3 clip.mp4
ffmpeg -loglevel error -f lavfi -i testsrc2=size=320x240:rate=25 -t 2 -c:v libx264 -pix_fmt yuv420p silent.mov
check joined.mp4            ffmpeg ffprobe -- tool join clip.mp4 silent.mov
check "clip parts (2)"      ffmpeg -- tool split --at 1,2.5 clip.mp4
check clip-redacted.mp4     ffmpeg -- tool redact --boxes "0,0,200,100" --style "#000000" --start 1 --end 3 clip.mp4
check clip-cropped.mp4      ffmpeg -- tool crop --box 10,10,301,201 clip.mp4
check clip-stereo.mp4       ffmpeg -- tool channels --count 2 --left 0 clip.mp4
check clip-adjusted.mp4     ffmpeg -- tool adjust --saturation 1.5 --sharpen 1 clip.mp4
check clip-tagged.mp4       ffmpeg -- tool tags --title "Zeidex test" clip.mp4
check voice-normalized.wav  ffmpeg -- tool normalize voice.wav
check voice-bleeped.wav     ffmpeg -- tool bleep --start 1 --end 2 voice.wav
check voice-visualizer.mp4  ffmpeg -- tool visualize voice.wav
check clip-frame-2s.png     ffmpeg -- tool snapshot --at 2 clip.mp4
check clip-clean.mp4        ffmpeg -- tool strip clip.mp4

check photo.webp            vips -- convert --to webp photo.png
check photo.pdf             vips -- convert --to pdf photo.png
check shot.png              vips ffmpeg -- convert --to png shot.bmp
check photo-resized.png     vips -- tool resize --max 300 photo.png
check photo-cropped.png     vips vipsheader -- tool crop --ratio 16:9 photo.png
check photo-framed.png      vips vipsheader -- tool background --color "#222" photo.png
check photo-compressed.png  vips -- tool compress --percent 50 photo.png
check images.pdf            vips -- tool images-to-pdf photo.png shot.bmp
check collage.jpg           vips -- tool collage photo.png photo.png
check photo-redacted.png    ffmpeg -- tool redact --boxes "10,10,200,100;300,200,50,50" photo.png
check photo-adjusted.png    ffmpeg -- tool adjust --contrast 1.3 --grain 8 photo.png
gs -q -dSAFER -dBATCH -dNOPAUSE -sDEVICE=png16m -r72 -sOutputFile=card.png doc.ps 2>/dev/null
check card-redacted.png     gs tesseract -- tool redact --boxes auto card.png

check "doc pages"           gs -- convert --to png doc.pdf
check doc.txt               gs -- convert --to txt doc.pdf
check doc-compressed.pdf    gs -- tool compress --preset low doc.pdf
check doc-ocr.txt           gs tesseract -- tool ocr --to txt --lang eng doc.pdf
check merged.pdf            qpdf -- tool merge-pdf doc.pdf other.pdf
check "doc pages (2)"       qpdf -- tool split-pdf doc.pdf
check doc-pages.pdf         qpdf -- tool pages --spec 2,1 doc.pdf
check doc-rotated.pdf       qpdf -- tool rotate doc.pdf
check "doc-pages (2).pdf"   qpdf -- tool pages --spec 2,1,1 --rotate 1:90,3:180 doc.pdf
check doc-clean.pdf         qpdf -- tool strip doc.pdf

check notes.docx            pandoc -- convert --to docx notes.md
check notes.html            pandoc -- convert --to html notes.md
check subs.vtt              -- convert --to vtt subs.srt
check folder.zip            7z -- tool archive folder
check archive.tar.gz        7z -- tool archive --format tgz folder subs.srt
check archive.zip           7z -- convert --to zip archive.tar.gz
check archive               7z -- tool extract archive.tar.gz

[[ -d "clip parts (2)" ]] && n=$(ls "clip parts (2)" | wc -l) && [[ $n -ne 3 ]] && { echo "FAIL  split --at gave $n parts, want 3"; fail=$((fail + 1)); }
[[ -f joined.mp4 ]] && ! ffprobe -v error -show_entries stream=width -of csv=p=0 joined.mp4 | grep -q 640 && { echo "FAIL  join did not use the first clip's canvas"; fail=$((fail + 1)); }
[[ -f clip-tagged.mp4 ]] && ! ffprobe -v error -show_entries format_tags=title -of csv=p=0 clip-tagged.mp4 | grep -q "Zeidex test" && { echo "FAIL  tags not written"; fail=$((fail + 1)); }
grep -qi "ocr test" doc-ocr.txt 2>/dev/null || { has tesseract && { echo "FAIL  OCR text mismatch"; fail=$((fail + 1)); }; }
echo "passed=$pass failed=$fail skipped=$skip"
[[ $fail -eq 0 ]]
