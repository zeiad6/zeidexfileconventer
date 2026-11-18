# Downloads the redistributable engines bundled into the Windows build (CI and local):
# FFmpeg/FFprobe (GPL build, BtbN), libvips (LGPL), qpdf (Apache-2.0), Ghostscript (AGPL,
# conda-forge), rhwp (MIT), Pandoc (GPL), 7-Zip (LGPL, pinned 26.03), LibreOffice and Calibre
# (MPL-2.0 / GPL-3.0), Tesseract (Apache-2.0), qrencode, ExifTool, ZBar, whisper.cpp,
# Real-ESRGAN and the two model files they need, plus the VC++ runtime their binaries link.
#
# LibreOffice is the reason this list grew: it used to be left to winget, so every document
# conversion ended with "LibreOffice not found" on a machine that had never installed it.
param(
  [ValidateSet("full", "lite")]
  [string]$Profile = $(if ($env:ZEIDEX_BUILD_PROFILE) { $env:ZEIDEX_BUILD_PROFILE } else { "full" })
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
Write-Host "==> engine profile: $Profile (windows-x64)"

$dst = Join-Path $PSScriptRoot "..\app\src-tauri\engines"
$tmp = Join-Path ([IO.Path]::GetTempPath()) "zeidex-engines"
New-Item -ItemType Directory -Force $dst, $tmp | Out-Null
# No Authorization header: every download is a public, checksummed file, and a token sent
# here would also reach conda.anaconda.org.
$headers = @{ "User-Agent" = "zeidex-ci" }

function Get-Pinned($url, $sha256, $name) {
  $file = Join-Path $tmp $name
  Invoke-WebRequest $url -OutFile $file -Headers $headers
  $actual = (Get-FileHash $file -Algorithm SHA256).Hash
  if ($actual -ne $sha256.ToUpper()) {
    throw "checksum mismatch for $name`: expected $sha256, got $actual"
  }
  return $file
}

# 7-Zip is needed to unpack the MSI/conda payloads below; the pinned copy is only used when the
# runner does not already have one, so the build never depends on the image.
$seven = "C:\Program Files\7-Zip\7z.exe"
if (-not (Test-Path $seven)) {
  $sevenZip = Get-Pinned `
    "https://github.com/ip7z/7zip/releases/download/26.03/7z2603-x64.msi" `
    "C0680064D698A62DD4A5A47F403DB356A6531A5473E4C4B1D090EA2590513926" `
    "7z2603-x64.msi"
  $sevenDir = Join-Path $tmp "7z-tool"
  Start-Process msiexec.exe -ArgumentList "/a", $sevenZip, "/qn", "TARGETDIR=$sevenDir" -Wait
  $seven = (Get-ChildItem $sevenDir -Recurse -Filter 7z.exe | Select-Object -First 1).FullName
}

# Unpacks a conda-forge package (.conda = zip holding one zstd tar) and returns its root.
function Get-CondaTree($url, $sha256, $name) {
  $pkg = Get-Pinned $url $sha256 $name
  $stage = Join-Path $tmp "$name-stage"
  $root = Join-Path $tmp "$name-root"
  Remove-Item $stage, $root -Recurse -Force -ErrorAction SilentlyContinue
  New-Item -ItemType Directory -Force $stage, $root | Out-Null
  & $seven x $pkg "-o$stage" -y | Out-Null
  $zst = Get-ChildItem $stage -Filter "pkg-*.tar.zst" | Select-Object -First 1
  if ($zst) {
    $tarStage = Join-Path $tmp "$name-tar"
    Remove-Item $tarStage -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force $tarStage | Out-Null
    & $seven x $zst.FullName "-o$tarStage" -y | Out-Null
    $tar = Get-ChildItem $tarStage -Filter "pkg-*.tar" | Select-Object -First 1
    & $seven x $tar.FullName "-o$root" -y | Out-Null
  } else {
    & $seven x (Get-ChildItem $stage -Filter "pkg-*.tar" | Select-Object -First 1).FullName "-o$root" -y | Out-Null
  }
  return $root
}

# Copies the tree that holds $exe into engines\$dest, so the executable sees its own resources.
# Installers wrap the real tree in PFiles/, Calibre2/ or program/; that wrapper is what ships.
function Copy-EngineTree($source, $exe, $dest) {
  $found = Get-ChildItem $source -Recurse -Filter $exe | Select-Object -First 1
  if (-not $found) { throw "$exe was not found after unpacking" }
  $dir = $found.Directory
  # Climb out of program/ (LibreOffice) so share/ travels with the binaries.
  if ($dir.Name -eq "program" -and $dir.Parent) { $dir = $dir.Parent }
  $target = Join-Path $dst $dest
  Remove-Item $target -Recurse -Force -ErrorAction SilentlyContinue
  New-Item -ItemType Directory -Force $target | Out-Null
  Copy-Item (Join-Path $dir.FullName "*") $target -Recurse -Force
}

# Releases usually arrive inside one wrapper folder (`realesrgan-ncnn-vulkan-20220424-windows/`).
# The app resolves `<engines>\<tool>\<exe>` — or one of its bin/, Library/bin/, program/ levels —
# so a wrapper would hide the binary from the lookup. Unwrap it while unpacking.
function Expand-Flat($archive, $target) {
  Remove-Item $target -Recurse -Force -ErrorAction SilentlyContinue
  Expand-Archive $archive $target -Force
  $items = @(Get-ChildItem $target)
  if ($items.Count -eq 1 -and $items[0].PSIsContainer) {
    $inner = $items[0].FullName
    Get-ChildItem $inner | Move-Item -Destination $target -Force
    Remove-Item $inner -Recurse -Force
  }
}


# snapshot as the Linux packages; "latest" would ship an unverified nightly of master.
$zip = Get-Pinned `
  "https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-09-30-13-08/ffmpeg-n9.0.2-17-g2a571b6068-win64-gpl-9.0.zip" `
  "A0E45723C72141975F51D8666302E614711745F3102B704CA3F82C897A58D278" `
  "ffmpeg-n9.0.2-win64-gpl.zip"
Expand-Archive $zip (Join-Path $tmp "ffmpeg") -Force
Get-ChildItem (Join-Path $tmp "ffmpeg") -Recurse -Include ffmpeg.exe, ffprobe.exe | Copy-Item -Destination $dst

# libvips: keep its bin/ folder intact, vips.exe needs the DLLs beside it.
$zip = Get-Pinned `
  "https://github.com/libvips/build-win64-mxe/releases/download/v8.18.7/vips-dev-x64-all-8.18.7.zip" `
  "78E56BA30C267BA32FF965200534BD5721364CADBEAD8C4D595E44B58DFEFF15" `
  "vips-dev-x64-all-8.18.7.zip"
Expand-Archive $zip (Join-Path $tmp "vips") -Force
$bin = Get-ChildItem (Join-Path $tmp "vips") -Recurse -Directory -Filter bin | Select-Object -First 1
New-Item -ItemType Directory -Force (Join-Path $dst "vips") | Out-Null
Copy-Item $bin.FullName (Join-Path $dst "vips") -Recurse -Force

# qpdf and remaining Full-only engines. In Lite mode only FFmpeg/FFprobe, libvips, 7-Zip and
# VC++ runtime are staged; heavy optional engines can be installed on demand from the UI.
if ($Profile -eq "full") {
# qpdf (pinned)
$zip = Get-Pinned `
  "https://github.com/qpdf/qpdf/releases/download/v12.4.2/qpdf-12.4.2-msvc64.zip" `
  "DB87077E683630C1217E0E8F9A20A9749D952AB676E881C3689187763A5DE25D" `
  "qpdf-12.4.2-msvc64.zip"
Expand-Archive $zip (Join-Path $tmp "qpdf") -Force
$bin = Get-ChildItem (Join-Path $tmp "qpdf") -Recurse -Directory -Filter bin | Select-Object -First 1
New-Item -ItemType Directory -Force (Join-Path $dst "qpdf") | Out-Null
Copy-Item $bin.FullName (Join-Path $dst "qpdf") -Recurse -Force

# Ghostscript (pinned conda-forge package). A .conda is a zip holding a zstd-compressed tar;
# 7-Zip unpacks both stages. The package expects to live in an env whose Library/ holds bin/
# and Init/, so the whole Library tree is copied rather than just the exe.
if (Test-Path $seven) {
  $conda = Get-Pinned `
    "https://conda.anaconda.org/conda-forge/win-64/ghostscript-10.08.0-hac47afa_0.conda" `
    "512BE9413C58B0AD3A2CD95A63EAEA73CBE32D9A6B9170C848BF825694A4ADB0" `
    "ghostscript-10.08.0-hac47afa_0.conda"
  $gs1 = Join-Path $tmp "gs1"; $gs2 = Join-Path $tmp "gs2"; $gs3 = Join-Path $tmp "gs3"
  & $seven x $conda "-o$gs1" -y | Out-Null
  $zst = Get-ChildItem $gs1 -Filter "pkg-*.tar.zst" | Select-Object -First 1
  & $seven x $zst.FullName "-o$gs2" -y | Out-Null
  $tar = Get-ChildItem $gs2 -Filter "pkg-*.tar" | Select-Object -First 1
  & $seven x $tar.FullName "-o$gs3" -y | Out-Null
  New-Item -ItemType Directory -Force (Join-Path $dst "gs") | Out-Null
  Copy-Item (Join-Path $gs3 "Library") (Join-Path $dst "gs") -Recurse -Force
} else {
  Write-Warning "7-Zip not found; Ghostscript is not bundled, so PDF rendering falls back to the system install"
}
}

# VC++ runtime: the conda Ghostscript and the Rust-built rhwp.exe link the dynamic CRT, and a
# clean Windows does not ship vcruntime140.dll. The DLLs go beside each consumer: gs.exe loads
# from its own folder, rhwp.exe from the engines root.
$vc = Get-Pinned `
  "https://conda.anaconda.org/conda-forge/win-64/vc14_runtime-14.44.35208-h818238b_41.conda" `
  "EC706FE9368B837FBD05AF4851F5EBEF12BEF8C471A273F4805F7AF788407317" `
  "vc14_runtime-14.44.35208-h818238b_41.conda"
if (Test-Path $seven) {
  $vc1 = Join-Path $tmp "vc1"; $vc2 = Join-Path $tmp "vc2"; $vc3 = Join-Path $tmp "vc3"
  & $seven x $vc "-o$vc1" -y | Out-Null
  $vzst = Get-ChildItem $vc1 -Filter "pkg-*.tar.zst" | Select-Object -First 1
  & $seven x $vzst.FullName "-o$vc2" -y | Out-Null
  $vtar = Get-ChildItem $vc2 -Filter "pkg-*.tar" | Select-Object -First 1
  & $seven x $vtar.FullName "-o$vc3" -y | Out-Null
  $vcbin = Join-Path $vc3 "Library\bin"
  Copy-Item (Join-Path $vcbin "*.dll") $dst -Force
  if (Test-Path (Join-Path $dst "gs\Library\bin")) {
    Copy-Item (Join-Path $vcbin "*.dll") (Join-Path $dst "gs\Library\bin") -Force
  }
}

# 7-Zip: pinned to the same 26.03 as the other platforms. Needed by both Lite and Full.
if (Test-Path $seven) {
  $msi = Get-Pinned `
    "https://github.com/ip7z/7zip/releases/download/26.03/7z2603-x64.msi" `
    "C0680064D698A62DD4A5A47F403DB356A6531A5473E4C4B1D090EA2590513926" `
    "7z2603-x64.msi"
  $zdir = Join-Path $tmp "7zmsi"
  & $seven x $msi "-o$zdir" -y | Out-Null
  Copy-Item (Join-Path $zdir "_7z.exe") (Join-Path $dst "7z.exe") -Force
  Copy-Item (Join-Path $zdir "_7z.dll") (Join-Path $dst "7z.dll") -Force
} else {
  Write-Warning "7-Zip not found; archives will rely on a system install"
}

if ($Profile -eq "full") {
# rhwp: the HWP/HWPX viewer-editor engine.
$rhwp = Get-Pinned `
  "https://github.com/edwardkim/rhwp/releases/download/v0.8.7/rhwp-v0.8.7-windows-x86_64.zip" `
  "F7AC326E98F595AE8FECD8DA7A614EE9A37DB77BDF938782A8E07C9DA473EC6F" `
  "rhwp-v0.8.7-windows-x86_64.zip"
Expand-Archive $rhwp (Join-Path $tmp "rhwp") -Force
Get-ChildItem (Join-Path $tmp "rhwp") -Recurse -Filter rhwp.exe | Select-Object -First 1 |
  Copy-Item -Destination $dst

# Pandoc: document conversion (DOCX, EPUB, Markdown, LaTeX…).
$pandoc = Get-Pinned `
  "https://github.com/jgm/pandoc/releases/download/3.12/pandoc-3.12-windows-x86_64.zip" `
  "2A77EBC2517D13E95056E76B1CD5B574CFE958AC61AA6058117D80C22CA19B79" `
  "pandoc-3.12-windows-x86_64.zip"
Expand-Archive $pandoc (Join-Path $tmp "pandoc") -Force
Get-ChildItem (Join-Path $tmp "pandoc") -Recurse -Filter pandoc.exe | Select-Object -First 1 |
  Copy-Item -Destination $dst

# --- tools that used to be "install it yourself" ---------------------------------------------

# LibreOffice: the document engine behind "PDF from Word/Excel/PowerPoint" and back. The MSI is
# big, but shipping it is what removes the "LibreOffice not found. Install: winget …" dead end.
#
# The cabinet holds every file flat, so 7-Zip cannot rebuild the program/ + share/ tree that
# soffice.exe expects. An administrative install (`msiexec /a`) is the documented way to unpack
# an MSI without installing it, and it restores the real layout.
$lo = Get-Pinned `
  "https://download.documentfoundation.org/libreoffice/stable/26.2.7/win/x86_64/LibreOffice_26.2.7_Win_x86-64.msi" `
  "480FECF759FC6393BD614FBF43753EB95A8198FC1A065D41B727E3B664CD1E59" `
  "LibreOffice_26.2.7_Win_x86-64.msi"
$lodir = Join-Path $tmp "libreoffice"
Remove-Item $lodir -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $lodir | Out-Null
$loRun = Start-Process msiexec.exe -ArgumentList "/a", "`"$lo`"", "/qn", "TARGETDIR=`"$lodir`"" -Wait -PassThru
if ($loRun.ExitCode -ne 0) { throw "LibreOffice administrative install failed with $($loRun.ExitCode)" }
Copy-EngineTree $lodir "soffice.exe" "libreoffice"
# soffice loads the dynamic CRT from its own folder once the tree has left Program Files.
Get-ChildItem $dst -Filter "*.dll" | Copy-Item -Destination (Join-Path $dst "libreoffice\program") -Force
# The MSI's Fonts\ folder is registered system-wide by the real installer, which an administrative
# install never does. Re-home it under share/fonts/truetype — a path LibreOffice scans — so Caladea
# and Carlito keep Calibri/Cambria metrics (page breaks stay put) and Amiri keeps Arabic shaping.
$loTree = Join-Path $dst "libreoffice"
# The admin install places Fonts\ beside LibreOffice\, but accept it inside the copied tree too:
# the layout is a Windows-side detail and a silent miss would only cost font fidelity.
$loMsiFonts = @((Join-Path $lodir "Fonts"), (Join-Path $loTree "Fonts")) |
  Where-Object { Test-Path $_ } | Select-Object -First 1
if ($loMsiFonts) {
  $loFonts = Join-Path $loTree "share\fonts\truetype"
  New-Item -ItemType Directory -Force $loFonts | Out-Null
  Copy-Item (Join-Path $loMsiFonts "*") $loFonts -Force
}
# The unpacked tree is 1.6 GB; the parts below are UI-only and headless `--convert-to` never
# touches them. Spelling dictionaries are the big one (about 470 MB for 58 languages), so only
# the two the app documents stay. Everything the filters read — share/registry, share/filter,
# share/config/soffice.cfg, share/fonts — is untouched.
Remove-Item (Join-Path $loTree "help"), (Join-Path $loTree "readmes"), (Join-Path $loTree "CREDITS.fodt") -Recurse -Force -ErrorAction SilentlyContinue
Get-ChildItem (Join-Path $loTree "share\extensions") -Directory -Filter "dict-*" |
  Where-Object { $_.Name -notin @("dict-en", "dict-ar") } |
  Remove-Item -Recurse -Force
Get-ChildItem (Join-Path $loTree "share\config") -Filter "images_*.zip" | Remove-Item -Force -ErrorAction SilentlyContinue
Write-Host "LibreOffice staged: $([math]::Round((Get-ChildItem $loTree -Recurse -File | Measure-Object Length -Sum).Sum / 1MB)) MB"

# Calibre: ebook conversion (EPUB/MOBI/AZW3 and PDF). Same administrative install: its MSI packs
# every file into opaque cabinet members, so only Windows can rebuild the tree.
$cal = Get-Pinned `
  "https://download.calibre-ebook.com/9.18.0/calibre-64bit-9.18.0.msi" `
  "05C0F2223866F8082E05B2D4BC9C57B31A833DE8355A1191FD79C260C2636D38" `
  "calibre-64bit-9.18.0.msi"
$caldir = Join-Path $tmp "calibre"
Remove-Item $caldir -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $caldir | Out-Null
$calRun = Start-Process msiexec.exe -ArgumentList "/a", "`"$cal`"", "/qn", "TARGETDIR=`"$caldir`"" -Wait -PassThru
if ($calRun.ExitCode -ne 0) { throw "Calibre administrative install failed with $($calRun.ExitCode)" }
Copy-EngineTree $caldir "ebook-convert.exe" "calibre"
# Qt's UI translations are 41 MB and this is a command-line engine; without them Qt falls back
# to English instead of failing to start.
Remove-Item (Join-Path $dst "calibre\app\translations") -Recurse -Force -ErrorAction SilentlyContinue
Write-Host "Calibre staged: $([math]::Round((Get-ChildItem (Join-Path $dst 'calibre') -Recurse -File | Measure-Object Length -Sum).Sum / 1MB)) MB"

# Tesseract OCR and qrencode ("Make QR code") share one conda environment.
#
# A conda package on its own is not a working engine: `tesseract.exe` links leptonica, libtiff,
# libarchive, libcurl, openjpeg, giflib, libwebp, libpng, liblzma, libjpeg-turbo and the VC
# runtime, and every one of those DLLs lives in a *dependency* package. Copying `Library\bin` out
# of the tesseract archive alone is what shipped an exe that died in the Windows loader with
# 0xC0000135 — the `exit Some(-1073741515)` users reported. qrencode was staged the same wrong
# way. The environment is resolved with micromamba (the tool the Unix script already uses) so the
# whole closure arrives, the solved `Library\bin` — conda's DLL directory on Windows — is staged
# once as `engines\ocr`, and both binaries are started before anything ships.
if (Test-Path $seven) {
  # micromamba 2.10.0-0, win-64: 11,723,264 bytes, digest from the published release asset.
  $mm = Get-Pinned `
    "https://github.com/mamba-org/micromamba-releases/releases/download/2.10.0-0/micromamba-win-64.exe" `
    "F8B4284BD4EB1134122EAF93B3EA6C7BE567E596DE5829E1A6F478E34E41974A" `
    "micromamba-win-64.exe"
  $mmRoot = Join-Path $tmp "mamba-root"
  $ocrEnv = Join-Path $tmp "ocr-env"
  Remove-Item $ocrEnv -Recurse -Force -ErrorAction SilentlyContinue
  # micromamba checks every package against the conda-forge repodata digest as it fetches it.
  & $mm create -y -r $mmRoot -p $ocrEnv -c conda-forge "tesseract=5.5.4" "libqrencode=4.1.1" | Out-Null
  $ocrBin = Join-Path $ocrEnv "Library\bin"
  foreach ($exe in "tesseract.exe", "qrencode.exe") {
    if (-not (Test-Path (Join-Path $ocrBin $exe))) { throw "micromamba staged no $exe" }
  }

  $ocr = Join-Path $dst "ocr\Library"
  $tedata = Join-Path $ocr "share\tessdata"
  Remove-Item (Join-Path $dst "ocr"), (Join-Path $dst "tesseract"), (Join-Path $dst "qrencode") `
    -Recurse -Force -ErrorAction SilentlyContinue
  New-Item -ItemType Directory -Force (Join-Path $ocr "bin"), $tedata | Out-Null
  Copy-Item (Join-Path $ocrBin "*") (Join-Path $ocr "bin") -Force

  # The package splits tessdata over two trees: the languages sit in `share/tessdata` at the env
  # prefix while `configs/` and `tessconfigs/` live under `Library/share/tessdata`. Search both
  # instead of assuming one layout.
  $tesTrees = @(Get-ChildItem $ocrEnv -Recurse -Directory -Filter "tessdata" -ErrorAction SilentlyContinue)
  $tesData = $tesTrees | Where-Object { Test-Path (Join-Path $_.FullName "eng.traineddata") } |
    Select-Object -First 1
  if (-not $tesData) { throw "eng.traineddata was not found in the Tesseract environment" }
  foreach ($lang in "eng", "ara", "osd") {
    $srcLang = Join-Path $tesData.FullName "$lang.traineddata"
    if (-not (Test-Path $srcLang)) { throw "$lang.traineddata was not found in $($tesData.FullName)" }
    Copy-Item $srcLang $tedata -Force
  }
  # `configs` (tsv/pdf/hocr output) and `tessconfigs` (language aliases) may sit in either tree;
  # pdf.ttf is not published in 5.5.4, so it is copied only when a future package carries it.
  foreach ($extra in "configs", "tessconfigs", "pdf.ttf") {
    $src = $tesTrees | ForEach-Object { Join-Path $_.FullName $extra } |
      Where-Object { Test-Path $_ } | Select-Object -First 1
    if ($src) { Copy-Item $src $tedata -Recurse -Force }
  }

  # Gate: an engine that cannot start must fail the build, not the user's first job. This is the
  # check the previous staging lacked, and it is the reason the missing DLL reached a release.
  foreach ($exe in "tesseract.exe", "qrencode.exe") {
    $probe = & (Join-Path $ocr "bin\$exe") --version 2>&1
    if ($LASTEXITCODE -ne 0) {
      throw "the staged $exe does not start (exit $LASTEXITCODE): $($probe -join ' ')"
    }
  }
  $langsOut = (& (Join-Path $ocr "bin\tesseract.exe") --tessdata-dir $tedata --list-langs 2>&1) -join "`n"
  if ($LASTEXITCODE -ne 0 -or $langsOut -notmatch "(?m)^eng\r?$" -or $langsOut -notmatch "(?m)^ara\r?$") {
    throw "staged Tesseract tessdata check failed in $tedata`: $langsOut"
  }
  Write-Host "Tesseract + qrencode staged: $((Get-ChildItem (Join-Path $ocr 'bin') -File | Measure-Object Length -Sum).Sum / 1MB) MB"
}

# ExifTool: the official Windows build is a packed exe, so no Perl is needed. `-k` pauses at the
# end of every run, which would hang each job, so the shipped copy is the plain exiftool.exe.
$exif = Get-Pinned `
  "https://sourceforge.net/projects/exiftool/files/exiftool-13.64_64.zip/download" `
  "72C29C44D1E860EEBCAEFA71692D979AF9858FE9A6D7D791A5E7F1DAC77804AF" `
  "exiftool-13.64_64.zip"
Expand-Flat $exif (Join-Path $dst "exiftool")
Rename-Item (Join-Path $dst "exiftool\exiftool(-k).exe") "exiftool.exe"

# ZBar: QR/barcode reading. The release archive keeps its DLLs beside zbarimg.exe.
$zbar = Get-Pinned `
  "https://github.com/mchehab/zbar/releases/download/0.23.95/zbar-win_x86_64-VfW.zip" `
  "40E01FB14CD8B02C99934A76A801EBD535C0A7F55D644A9D8FEDE0B71253AE09" `
  "zbar-win_x86_64-VfW.zip"
$zbardir = Join-Path $tmp "zbar"
Expand-Flat $zbar $zbardir
Remove-Item (Join-Path $dst "zbar") -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force (Join-Path $dst "zbar\bin") | Out-Null
Copy-Item (Join-Path $zbardir "bin\*.exe") (Join-Path $dst "zbar\bin") -Force
Copy-Item (Join-Path $zbardir "bin\*.dll") (Join-Path $dst "zbar\bin") -Force -ErrorAction SilentlyContinue

# whisper.cpp: the x64 CPU build still names its CLI main.exe; the app looks for whisper-cli.exe.
$wh = Get-Pinned `
  "https://github.com/ggml-org/whisper.cpp/releases/download/b5454/whisper-bin-x64.zip" `
  "6BA69E3482D7826214F90A6A9C84CA07782AEC1E1D0C6A7C30C994FD5D816CCB" `
  "whisper-bin-x64.zip"
$whdir = Join-Path $tmp "whisper"
Remove-Item $whdir -Recurse -Force -ErrorAction SilentlyContinue
Expand-Archive $wh $whdir -Force
Remove-Item (Join-Path $dst "whisper") -Recurse -Force -ErrorAction SilentlyContinue
$whBin = Join-Path $dst "whisper\bin"
New-Item -ItemType Directory -Force $whBin | Out-Null
$whrel = Get-ChildItem $whdir -Recurse -Directory -Filter Release | Select-Object -First 1
$whsrc = if ($whrel) { $whrel.FullName } else { (Get-ChildItem $whdir -Directory | Select-Object -First 1).FullName }
# b5454 ships `whisper-cli.exe` and a legacy `main.exe` alias; older builds ship only `main.exe`.
# The app runs whisper-cli, so take that one and leave the other twenty executables behind.
if (Test-Path (Join-Path $whsrc "whisper-cli.exe")) {
  Copy-Item (Join-Path $whsrc "whisper-cli.exe") $whBin -Force
} else {
  Copy-Item (Join-Path $whsrc "main.exe") (Join-Path $whBin "whisper-cli.exe") -Force
}
Copy-Item (Join-Path $whsrc "*.dll") $whBin -Force -ErrorAction SilentlyContinue

# Real-ESRGAN: AI upscale. Its networks live in models/ beside the executable.
$esr = Get-Pinned `
  "https://github.com/xinntao/Real-ESRGAN/releases/download/v0.2.5.0/realesrgan-ncnn-vulkan-20220424-windows.zip" `
  "ABC02804E17982A3BE33675E4D471E91EA374E65B70167ABC09E31ACB412802D" `
  "realesrgan-ncnn-vulkan-20220424-windows.zip"
Expand-Flat $esr (Join-Path $dst "realesrgan")
# The release carries two sample photos next to the binary; only the exe and models/ ship.
Get-ChildItem (Join-Path $dst "realesrgan") -File | Where-Object { $_.Extension -in ".jpg", ".jpeg", ".png" } | Remove-Item -Force

# Models shared by every platform: whisper's tiny network and rembg's U²-Net.
$modelDir = Join-Path $dst "models"
New-Item -ItemType Directory -Force $modelDir | Out-Null
$tiny = Get-Pinned `
  "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin" `
  "BE07E048E1E599AD46341C8D2A135645097A538221678B7ACDD1B1919C6E1B21" `
  "ggml-tiny.bin"
Copy-Item $tiny (Join-Path $modelDir "ggml-tiny.bin") -Force
$u2 = Get-Pinned `
  "https://github.com/danielgatis/rembg/releases/download/v0.0.0/u2net.onnx" `
  "8D10D2F3BB75AE3B6D527C77944FC5E7DCD94B29809D47A739A7A728A912B491" `
  "u2net.onnx"
Copy-Item $u2 (Join-Path $modelDir "u2net.onnx") -Force
}

# NSIS builds one data block and its compiler cannot map beyond 2 GB, so a 2.7 GB resource
# tree fails with "error mmapping datablock". Ship the engines as one solid archive instead:
# the installer copies 7z.exe + engines.7z, and the install hook below unpacks it in place.
# 7z.exe and 7z.dll stay outside the archive, because they are what performs the extraction.
# Packaging only: a plain run keeps the tree loose so `tauri dev` still finds every engine.
if ($env:ZEIDEX_PACK_ENGINES -eq "1") {
$sevenBundled = Join-Path $dst "7z.exe"
if (-not (Test-Path $sevenBundled)) { throw "the bundled 7-Zip was not staged" }
$pack = Join-Path $dst "engines.7z"
Remove-Item $pack -Force -ErrorAction SilentlyContinue
Push-Location $dst
& $seven a -t7z -mx=9 -ms=on "-mmt=on" $pack "*" "-x!7z.exe" "-x!7z.dll" "-x!engines.7z" | Out-Null
$packCode = $LASTEXITCODE
Pop-Location
if ($packCode -ne 0) { throw "7-Zip failed to pack the engines ($packCode)" }
Get-ChildItem $dst -Force |
  Where-Object { $_.Name -notin @("7z.exe", "7z.dll", "engines.7z") } |
  Remove-Item -Recurse -Force
Write-Host "Engines packed: $([math]::Round((Get-Item $pack).Length / 1MB)) MB"
Write-Host "7z.exe deps: $((Get-ChildItem $dst -File | Measure-Object).Count) files beside the archive"
} else {
  Write-Host "Engines staged loose (set ZEIDEX_PACK_ENGINES=1 for a bundle build)"
}
