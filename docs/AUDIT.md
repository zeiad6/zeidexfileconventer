# Zeidex Convert — source audit and upgrade log

Scope: the Android app in `android/` (package `com.zeidex.convert`), the Rust core/CLI in
`crates/`, and the Tauri desktop shell in `app/`. Every Kotlin and Rust source file was read,
not just the READMEs. Findings are grouped by severity; each one names the fix that shipped.

## 1. Correctness and honesty (high)

| # | Finding | Fix |
|---|---|---|
| A1 | **MP3 extraction wrote an `.m4a` file.** `MediaAudioExtractor` copied the AAC track and named the output `.mp3`, so players and services saw the wrong container. | The Android engine now reports `m4a` (the real container) and the UI/format table says so. Real MP3 encoding stays on the desktop app, where FFmpeg is bundled. |
| A2 | **"Upscale to 1080p" and "bitrate boost" changed nothing.** `VideoEnhancer` copied compressed samples and rewrote the header, so the output kept the source resolution and bitrate. | Those modes were removed. `RemuxMode::FAST` and `STRIP_METADATA` are real container operations; a request that needs a re-encode returns an explicit `Unsupported` error that points at the desktop app. |
| A3 | **Conversions could leave half-written files** when the process died or the user cancelled: engines wrote straight to the destination. | `AtomicWriter` stages every output in a hidden `.zeidex-*` file in the target directory and commits with a rename; failures delete the staging file. |
| A4 | **Name collisions overwrote or blocked output.** `File.createNewFile` style checks raced and produced `name (1).ext` inconsistently. | `AtomicWriter`/`FileSafety.uniqueName` produce `name 2.ext`, `name 3.ext`, … deterministically, and the commit is exclusive. |
| A5 | **Cancellation kept engines running.** Coroutine cancellation threw `CancellationException` inside the queue loop and could be swallowed by `catch (e: Exception)`. | The queue rethrows `CancellationException`, engines call `ensureActive()` between entries and before committing, and every engine runs on `Dispatchers.IO`. |
| A6 | **SRT/VTT parsing produced empty output.** `parseTiming` sliced the end timestamp with `substringBefore(' ')` before trimming, and every cue line has a space after `-->`, so the end time parsed as an empty string and the cue was dropped. Every subtitle conversion silently wrote a header-only file. | `parseTiming` trims the field before slicing. `SubtitleConverterTest` covers SRT→VTT, VTT→SRT (short timestamps and cue settings), ASS round-trip, plain text, CRLF/BOM, unknown-extension sniffing and the end-before-start clamp. |

## 2. Security (high)

| # | Finding | Fix |
|---|---|---|
| S1 | **Zip-Slip / path traversal in archive extraction.** Entry names were joined to the destination without normalisation, so `../../x` or an absolute path escaped the folder. | `FileSafety.resolveInside` canonicalises the destination, rejects absolute entries, `..` segments, NUL bytes and Windows reserved names, and re-checks the parent after resolution. Applied to ZIP, TAR, TAR.GZ and GZ. |
| S2 | **Archive bombs.** No limit on entry count or total uncompressed size; a 42.zip style archive could fill the device. | `ArchiveConverter` enforces an entry budget (10 000) and a byte budget (2 GiB), counts bytes actually written, and aborts with a clear message. |
| S3 | **Symlink/hardlink entries** in TAR could be recreated and later used to write outside the destination. | `TarArchive` skips link and device entries entirely and never creates them. |
| S4 | **Metadata leaked** in outputs: GPS, camera serial and timestamps survived every image/video conversion. | `MetadataStripper` removes EXIF/XMP/ICC-bearing chunks from JPEG, PNG and WebP without re-encoding; `VideoEnhancer` drops `location`/`creation-time`; the UI exposes a «strip metadata» switch. |

## 3. Missing capability (medium)

| # | Finding | Fix |
|---|---|---|
| C1 | PDF was read-only (text extraction); no document output. | `DocumentConverter` uses `pdfbox-android` 2.0.27.0 for text and table extraction and writes DOCX (hand-written OOXML in `DocxWriter`, no new dependency) or TXT/Markdown. Scanned PDFs fall back to OCR through Tesseract when the desktop engine is available; the app states when it is not. |
| C2 | Subtitle formats were not handled at all. | `SubtitleConverter` reads/writes SRT, WebVTT (including short timestamps and cue settings) and ASS, plus plain text, with timing preserved. No external engine. |
| C3 | Image tools only resized by width, with no crop. | Aspect crops (1:1, 16:9, 4:3), resolution presets (1920, 1080, 50 %) and `inSampleSize` downsampling so large photos do not allocate full bitmaps. |
| C4 | Progress was all-or-nothing. | Engines report 0–100 % into `ConversionForegroundService`; the notification and the card progress bar follow the same value. |

## 4. Code quality (low)

- `Format.kt` now models a `SUBTITLE` category with honest target lists per category, so the UI
  cannot offer a conversion that no engine implements.
- `StorageManager` uses `FileSafety` for every destination instead of ad-hoc `File(...)` joins.
- `MotionPhotoExtractor` validates the extracted MP4 (`ftyp` box) before committing, so a broken
  motion photo yields an error instead of a corrupt file.
- `TarArchive` now skips the padding block of GNU long-name and pax headers; previously the
  stream desynchronised on any archive that used them.
- `VideoEnhancer` no longer calls the non-existent `MediaFormat.getValue`; it probes the typed
  getters, which is the only type-erased read the platform offers.

## 5. Desktop (Rust + Tauri)

- **Drag detection** moved from an `rdev` low-level hook to a 60 Hz `GetAsyncKeyState` poll on
  Windows (`win_drag_hook.rs`), with `rdev` kept for X11. `CF_HDROP` is read as a best-effort
  probe; the authoritative list comes from Tauri's OLE drop target.
- **Wheel** rebuilt as three concentric rings (modes / items / nested options) with mode
  morphing on scroll and dwell, a cancel hub, and keyboard navigation. Heterogeneous selections
  no longer produce an empty ring: `format::mixed_targets` returns the union of what each file
  can become, ordered by coverage and capped at eight segments.
- **Overlay windows** get `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST` so the wheel and
  the orb never steal focus or appear in Alt+Tab.
- **CLI** renamed to `zeidex-convert`, gains `--quality 1-100` on `convert`, `--version`, and the
  `strip-exif` / `strip-metadata` aliases.
- **Offline guarantee** is enforced by tests: no networking crate may appear in a manifest, and a
  real conversion must not open a socket (`crates/core/tests/offline.rs`).
- **Extraction safety** is enforced by a test that writes a ZIP by hand with `../../escape.txt`
  and `/escape.txt` entries, runs the real extraction pipeline, and walks the scratch tree to
  assert nothing landed outside the archive's own folder
  (`crates/core/tests/extract_safety.rs`; skipped when no 7-Zip binary is installed).
- **Backdrop** (`win_studio.rs`): the studio window asks for Mica at creation and the build
  script then applies the ladder explicitly — Mica above build 22621, Acrylic above 17763, plain
  blur below — plus a theme-matched dark frame and rounded corners. The page paints a
  near-opaque sheet so it stays readable without a compositor.
- **Studio shell**: the main window is a sidebar (Converter, Studio Tools, Floating shelf, Watch
  folders, Settings) over the same picker; the shelf lists what the orb holds and can open it as
  one batch; watch rules have their own summary that jumps into the editor. The window is
  undecorated: the header carries macOS-style close/minimise/zoom buttons, `data-tauri-drag-region`
  moves it, and eight edge strips call `startResizeDragging`, so the glass surface has no OS
  chrome over it.
- **Actions ring**: any file, mixed drags included — float to shelf, archive to ZIP, extract,
  batch rename (opens the pattern form with its preview in the studio window), SHA-256, reveal,
  copy path, move, terminal.
- **Android power tools**: the same centred crops (1:1, 16:9, 4:3, 9:16) and size presets
  (1920 px, 1080 px, 50 %) exist on both the home screen and the studio tools screen, and both
  paths reach `ImageConverter`'s `inSampleSize` decoding.
- **Desktop UI is React 19 + TypeScript + Tailwind CSS v4 + Framer Motion** (`app/src`, Vite,
  five HTML entries because Tauri opens five windows). Tailwind v4's `@import "tailwindcss"` and
  `@theme inline` tokens carry layout and utility styling; the wheel's SVG rings, the orb
  gradient, the glass sheet, the eight resize strips and the modal internals live in
  `@layer components` in `app/src/styles.css`, because they are generated geometry
  (`arcPath`) rather than utility-class material — a deliberate split, not an unfinished port.
  Framer Motion animates the ring swap and the sub-ring, which is what makes the wheel read as
  one wheel morphing instead of a redraw.
- **The UI contract is tested, not eyeballed**: `app/test` runs 48 Vitest specs (159 assertions)
  in jsdom against a fake `window.__TAURI__`, covering the same ground as the previous 73
  plain-`node` assertions — ring segment counts per mode, dwell-opened sub-rings, mode keys,
  keyboard/click paths, hub cancel, preview-pixel → source-pixel editor maths, crop aspect,
  organizer/join ordering, pending-frame cancellation, settings validation, recipes, watch
  rules, rail routing, traffic lights and resize strips, shelf drag-out, and job rows. The ids
  the old build exposed (`#grid`, `#tabs`, `#form`, `#sel`, `#view-*`, `#nav*`, `#win*`,
  `#orb`, `#badge`, `#params`, `#wheel`) are preserved as the automation contract, and one spec
  asserts the whole list against the rendered DOM so the contract cannot rot silently.

## 6. Known gaps

- Windows code is cross-checked on the Linux host with `cargo check --target
  x86_64-pc-windows-msvc`. The Windows resource step (icon, manifest) needs the Windows SDK, so
  `build.rs` skips only that step for a cross-target check; the native `windows-latest` CI job
  still embeds it.
- Name collisions use `name (2).ext` on the desktop and `name 2.ext` on Android. Both never
  overwrite; only the separator differs.
- The React build adds a Node step to CI (`npm ci && npm test && npm run build` in `app`) and
  puts `app/ui-dist` in the bundle: about 394 kB of JS and 30 kB of CSS — 134 kB gzipped — most
  of it React and ReactDOM, against a hand-written UI that needed no toolchain. The trade was
  made for typed IPC calls, one design system and component-level tests; the installer still
  ships fully offline.
- One deliberate behaviour change in the port: a finished job row shows a 100 % bar instead of
  freezing at the last progress event (the old row stayed at, say, 60 % while marked done).
  `app/test/jobs.test.tsx` asserts the new value.
- Android cannot encode MP3/WAV/FLAC/OGG or transcode video; those actions return a message that
  names the desktop app instead of pretending to work.
- Android has no OCR model: a scanned PDF (no text layer) returns "OCR is available in the
  desktop app" instead of an empty .docx. Bundling an OCR engine would add several MB and a
  model download, which conflicts with the offline-first APK; the desktop app runs Tesseract.
  Text-layer PDF→DOCX does reconstruct simple tables from aligned columns.
