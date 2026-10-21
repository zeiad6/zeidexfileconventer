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
# ---- v0.4: beyond Tangerine ----
cp photo.png logo.png
check photo-watermarked.png ffmpeg ffprobe -- tool watermark --image logo.png --pos tr --scale 15 photo.png
check clip-watermarked.mp4  ffmpeg ffprobe -- tool watermark --image logo.png --opacity 0.5 clip.mp4
check photo.ico             ffmpeg -- tool icon photo.png
check slideshow.mp4         ffmpeg ffprobe -- tool slideshow --seconds 1 photo.png shot.bmp
check clip-faded.mp4        ffmpeg ffprobe -- tool fade --seconds 0.5 clip.mp4
check clip-reversed.mp4     ffmpeg ffprobe -- tool reverse clip.mp4
check clip-x2.mp4           ffmpeg -- tool loop --count 2 clip.mp4
check clip-stabilized.mp4   ffmpeg -- tool stabilize clip.mp4
check "clip frames"         ffmpeg -- tool frames --fps 2 clip.mp4
check clip-subtitled.mp4    ffmpeg -- tool add-subs clip.mp4 subs.srt
cp clip-subtitled.mp4 withsubs.mp4
check withsubs.srt          ffmpeg -- tool extract-subs withsubs.mp4
check "clip-subtitled (2).mp4" ffmpeg -- tool burn-subs clip.mp4 subs.srt
check subs-shifted.srt      -- tool shift-subs --seconds 2 subs.srt
check clip.mp4.sha256       -- tool checksum clip.mp4
check clip-faded-trimmed.mp4 ffmpeg ffprobe -- recipe "fade seconds=0.5 | trim start=1 end=3" clip-faded.mp4
if has ffmpeg; then
  cp voice.wav a1.wav; cp voice.wav a2.wav; cp voice.wav a3.wav
  if "$Z" convert --to mp3 --jobs 3 a1.wav a2.wav a3.wav >/dev/null 2>err.log && [[ -s a1.mp3 && -s a2.mp3 && -s a3.mp3 ]]; then
    echo "ok    convert --jobs 3 (parallel batch)"; pass=$((pass + 1))
  else echo "FAIL  parallel batch"; fail=$((fail + 1)); fi
  mkdir inbox
  "$Z" watch inbox --ext wav --do "convert to=flac" >watch.log 2>&1 &
  wpid=$!
  sleep 1; cp voice.wav inbox/new.wav
  for _ in $(seq 1 15); do [[ -s inbox/new.flac ]] && break; sleep 1; done
  kill $wpid 2>/dev/null
  if [[ -s inbox/new.flac ]]; then echo "ok    watch folder"; pass=$((pass + 1)); else echo "FAIL  watch folder"; cat watch.log; fail=$((fail + 1)); fi
fi
mkdir ren && touch ren/x.txt ren/y.txt
if "$Z" tool rename --pattern "note-{n:2}" ren/x.txt ren/y.txt >/dev/null && [[ -f ren/note-01.txt && -f ren/note-02.txt ]]; then
  echo "ok    batch rename"; pass=$((pass + 1))
else echo "FAIL  batch rename"; fail=$((fail + 1)); fi
[[ -f clip.mp4.sha256 ]] && has sha256sum && ! sha256sum -c clip.mp4.sha256 >/dev/null 2>&1 && { echo "FAIL  checksum does not verify with sha256sum"; fail=$((fail + 1)); }
[[ -f photo.ico ]] && has file && ! file photo.ico | grep -qi "icon" && { echo "FAIL  ico not recognised"; fail=$((fail + 1)); }
# ---- v0.5 ----
ffmpeg -loglevel error -f lavfi -i testsrc=size=320x240 -frames:v 1 cam.jpg
check cam-exif.jpg          exiftool -- tool exif --artist "Zeidex QA" --gps "24.7136,46.6753" cam.jpg
[[ -f cam-exif.jpg ]] && ! exiftool -s3 -Artist cam-exif.jpg | grep -q "Zeidex QA" && { echo "FAIL  exif artist not written"; fail=$((fail + 1)); }
check cam-exif-clean.jpg    exiftool -- tool strip cam-exif.jpg
[[ -f cam-exif-clean.jpg ]] && exiftool -s3 -GPSLatitude cam-exif-clean.jpg | grep -q . && { echo "FAIL  strip left GPS behind"; fail=$((fail + 1)); }
check photo-9x16.png        ffmpeg -- tool social photo.png
check clip-1x1.mp4          ffmpeg -- tool social --ratio 1:1 clip.mp4
check clip-sheet.jpg        ffmpeg ffprobe -- tool contact-sheet clip.mp4
check clip-360p.mp4         ffmpeg -- tool video-size --height 360 clip.mp4
check clip-newaudio.mp4     ffmpeg -- tool replace-audio clip.mp4 voice.wav
check voice-denoised.wav    ffmpeg -- tool denoise-audio voice.wav
check voice-nosilence.wav   ffmpeg -- tool remove-silence voice.wav
check "photo web"           vips -- tool web-set photo.png
check "clip parts (3)"      7z -- tool archive --password s3cret --split 100KB clip.mp4
check subs.7z               7z -- tool archive --format 7z --password s3cret subs.srt
mv subs.7z secret.7z 2>/dev/null
check secret                7z -- tool extract --password s3cret secret.7z
if has 7z && [[ -f secret.7z ]] && "$Z" tool extract secret.7z >/dev/null 2>&1; then echo "FAIL  extract without password succeeded"; fail=$((fail + 1)); fi
mkdir dupes && cp subs.srt dupes/a.srt && cp subs.srt dupes/b.srt
if "$Z" tool duplicates dupes 2>/dev/null | grep -q "2 copies"; then echo "ok    duplicates"; pass=$((pass + 1)); else echo "FAIL  duplicates"; fail=$((fail + 1)); fi
if has qrencode && has zbarimg; then
  "$Z" qr-make "https://zeidex.example/قيمة" --out code.png >/dev/null
  if "$Z" tool qr-read code.png | grep -q "zeidex.example"; then echo "ok    qr-make + qr-read"; pass=$((pass + 1)); else echo "FAIL  qr round trip"; fail=$((fail + 1)); fi
else
  echo "SKIP  qr round trip (no qrencode/zbarimg)"; skip=$((skip + 1))
fi
grep -qi "ocr test" doc-ocr.txt 2>/dev/null || { has tesseract && { echo "FAIL  OCR text mismatch"; fail=$((fail + 1)); }; }
echo "passed=$pass failed=$fail skipped=$skip"
[[ $fail -eq 0 ]]
