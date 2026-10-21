# Tangerine File Converter — clean-room reverse spec

Method: behavioural / spec mining only. The Mac App Store binary is FairPlay-encrypted and
there is no direct download, so no binary was disassembled or copied. Every line below comes
from public material:

- [S1] App Store listing — https://apps.apple.com/us/app/tangerine-file-converter/id6813034065 (v2.0.1)
- [S2] Product site — https://tangerineformac.com/
- [S3] Tools page — https://tangerineformac.com/tools

Facts are marked **[obs]**; guesses are marked **[inf]**.

## 1. Product shape

- [obs][S1] macOS 13+, Apple silicon, 97.6 MB, menu-bar app, one-time $29.99 unlock after 20 free files.
- [obs][S2] 275 conversion options, 26 file tools, 6 file actions. All processing is local.
- [inf] The 97.6 MB size points to bundled media engines (an FFmpeg build, plus Apple frameworks for images and PDF).

## 2. Interaction model (EARS)

- **R1** [obs][S1,S2] While the user drags a file in Finder, when the user taps Shift, the system shall show a radial wheel of output formats around the pointer.
- **R2** [obs][S2] While the wheel is open, when the user taps Command, the system shall switch the wheel to file tools. When the user taps Control, it shall switch to file actions.
- **R3** [obs][S2] The wheel shall stay open after the key is released, until the user drops the file or cancels.
- **R4** [obs][S1] When the file is dropped on a format, the system shall save the converted copy next to the original and never modify the original.
- **R5** [obs][S1] When several compatible files are dropped, the system shall process all of them in one job.
- **R6** [obs][S1] When a job runs, the system shall show a small progress window and reveal the finished files in Finder.
- **R7** [obs][S2] When the user presses Enter on a selected file, the system shall open the format menu. Arrow keys shall choose an item, Enter shall apply it, and Escape shall cancel.
- **R8** [obs][S2] Where the user changes the trigger keys in Settings, the system shall use the new keys.

## 3. Formats [obs][S2]

| Group | In | Out / notes |
|---|---|---|
| Images | JPG PNG WebP HEIC TIFF AVIF BMP SVG | same set + PDF. SVG is input only |
| Audio | MP3 M4A WAV FLAC OGG Opus AIFF WMA | same set |
| Video | MP4 MOV MKV WebM AVI WMV GIF | same set + MP3 |
| PDF | PDF | DOCX PAGES PPTX XLSX JPG PNG EPUB TXT MD. All pages, images at 300 DPI |
| Documents | DOCX PAGES MD TXT EPUB RTF | same set + HTML |
| Slides | PPT PPTX | PDF JPG PNG |
| Spreadsheets | XLSX XLS ODS CSV TSV | same set + PDF |
| Subtitles | SRT VTT | SRT VTT TXT |
| Archives | ZIP TAR GZIP RAR | ZIP TAR GZIP. RAR is input only |

## 4. Tools [obs][S3]

Any file: compress (to a target size, a % reduction, or a preset, with optional resize), edit or remove metadata.
Images: edit (exposure, colour, dehaze, grain), annotate, add a background, crop (ratio or exact pixels), redact (face/text detection), make a collage, make a PDF from images.
Video: trim, crop (keeps the audio), change speed (keeps the pitch, works on silent video), join (output is MP4, the first clip sets the canvas), save frames as PNG, split (at points or into equal parts, output to one folder), redact.
Audio: normalize (loudness range and true peak), make a visualizer MP4 (landscape, portrait or square), trim (optionally cut silence at both ends), change channels (mono or stereo), bleep a range.
PDF: merge (keeps the selection order), organize pages (reorder, rotate, duplicate, remove).
Actions [S2]: AirDrop, Float shelf, Copy, Move, Copy Path, open in Terminal. Actions do not count against the free allowance.

## 5. Uncertainties

- U1: The site does not say whether HEIC and AVIF *encoding* is native (ImageIO) or bundled. [inf] It is native.
- U2: The site does not say how DOCX, PPTX and XLSX are produced from PDF. [inf] It uses an internal converter, because no LibreOffice runtime fits in 97.6 MB.
- U3: The site does not say how face and text detection works. [inf] It uses Apple Vision, which has no direct counterpart on Windows or Linux.

## 6. Mapping to Zeidex Convert (Windows + Linux)

| Tangerine | Zeidex |
|---|---|
| Shift / Cmd / Ctrl while dragging | Global hook (`rdev`): Shift = convert, Alt = tools, Ctrl = actions. Works on Windows and X11. On Wayland, where global hooks are blocked, use the floating orb (shelf) or the main window |
| Wheel | Transparent always-on-top Tauri window that is a native drop target. Hover over a segment selects it. Dwelling on a mode chip switches the mode, so it also works without a keyboard |
| ImageIO / AVFoundation / PDFKit | libvips, FFmpeg, qpdf, Ghostscript |
| Office / Pages / EPUB | LibreOffice headless, Pandoc, Calibre (`ebook-convert`) |
| Vision text detection | Tesseract OCR: searchable PDF or TXT from scans and photos (not in Tangerine) |
| PDF → EPUB (missing; a reviewer asked for it) | Calibre `ebook-convert` |
| Finder "reveal" | Explorer `/select`, or the freedesktop `FileManager1.ShowItems` D-Bus call |
| Services menu | Windows "Send To" shortcut, Nautilus script, Nemo action, Dolphin service menu |
| — | CLI `zeidex` for scripting and batch work (not in Tangerine) |

Implemented in v0.2: a visual editor window for crop, redact (blur or solid colour, with a
time range for video) and annotate (box, arrow, text, numbered callout, highlighter). Also:
join uses the first clip's canvas, split at exact points, separate left/right levels for
channels, and remappable wheel keys (R8).

Implemented in v0.3: colour adjust (eq, hue, sharpen, denoise, grain) with live preview,
automatic text redaction through Tesseract word boxes, annotations composited onto the
original at full resolution, a visual PDF page organizer (reorder, rotate, duplicate, remove),
PDF metadata removal, audio/video tag editing, and the Float shelf with native drag-out
(tauri-plugin-drag).

Beyond Tangerine in v0.4: parallel batches, recipes (tool chains, saved in Settings and
shown in the Tools wheel), watch folders, batch rename, SHA-256 checksums, logo watermarks,
local speech-to-SRT with whisper.cpp, subtitle shift/extract/add/burn, video reverse, loop,
stabilize, frame export, image slideshows, fades, PDF password protect/unlock, and
multi-size .ico export.

Still out of scope: face detection (no portable engine in the chosen stack; U3), dehaze,
EXIF editing (removal only), and R7 (Enter on a file manager selection).
