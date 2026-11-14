# Downloads the redistributable engines bundled into the Windows build (CI and local):
# FFmpeg/FFprobe (GPL build, BtbN), libvips (LGPL), qpdf (Apache-2.0), Ghostscript (AGPL,
# conda-forge), rhwp (MIT), Pandoc (GPL) and 7-Zip (LGPL, pinned 26.03), plus the VC++ runtime
# their dynamically linked binaries need.
# LibreOffice, Calibre and Tesseract are NOT bundled: the app's Engines panel shows the winget
# command for each one.
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$dst = Join-Path $PSScriptRoot "..\app\src-tauri\engines"
$tmp = Join-Path ([IO.Path]::GetTempPath()) "zeidex-engines"
New-Item -ItemType Directory -Force $dst, $tmp | Out-Null
$headers = @{ "User-Agent" = "zeidex-ci" }
if ($env:GITHUB_TOKEN) { $headers.Authorization = "Bearer $env:GITHUB_TOKEN" }

function Get-Asset($repo, $pattern) {
  $rel = Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest" -Headers $headers
  $asset = $rel.assets | Where-Object { $_.name -match $pattern } | Select-Object -First 1
  if (-not $asset) { throw "no asset matching $pattern in $repo" }
  $file = Join-Path $tmp $asset.name
  Invoke-WebRequest $asset.browser_download_url -OutFile $file -Headers $headers
  return $file
}

function Get-Pinned($url, $sha256, $name) {
  $file = Join-Path $tmp $name
  Invoke-WebRequest $url -OutFile $file -Headers $headers
  $actual = (Get-FileHash $file -Algorithm SHA256).Hash
  if ($actual -ne $sha256.ToUpper()) {
    throw "checksum mismatch for $name`: expected $sha256, got $actual"
  }
  return $file
}

# FFmpeg + FFprobe (BtbN static builds: no DLL folder to carry).
$zip = Get-Asset "BtbN/FFmpeg-Builds" '^ffmpeg-master-latest-win64-gpl\.zip$'
Expand-Archive $zip (Join-Path $tmp "ffmpeg") -Force
Get-ChildItem (Join-Path $tmp "ffmpeg") -Recurse -Include ffmpeg.exe, ffprobe.exe | Copy-Item -Destination $dst

# libvips: keep its bin/ folder intact, vips.exe needs the DLLs beside it.
$zip = Get-Asset "libvips/build-win64-mxe" '^vips-dev-x64-all-[\d.]+\.zip$'
Expand-Archive $zip (Join-Path $tmp "vips") -Force
$bin = Get-ChildItem (Join-Path $tmp "vips") -Recurse -Directory -Filter bin | Select-Object -First 1
New-Item -ItemType Directory -Force (Join-Path $dst "vips") | Out-Null
Copy-Item $bin.FullName (Join-Path $dst "vips") -Recurse -Force

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
$seven = "C:\Program Files\7-Zip\7z.exe"
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

# VC++ runtime: the conda Ghostscript and the Rust-built rhwp.exe link the dynamic CRT, and a
# clean Windows does not ship vcruntime140.dll. The DLLs go beside each consumer: gs.exe loads
# from its own folder, rhwp.exe from the engines root.
$vc = Get-Pinned `
  "https://conda.anaconda.org/conda-forge/win-64/vc14_runtime-14.44.35208-h818238b_41.conda" `
  "EC706FE9368B837FBD05AF4851F5EBEF12BEF8C471A273F4805F7AF788407317" `
  "vc14_runtime-14.44.35208-h818238b_41.conda"
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

# 7-Zip: pinned to the same 26.03 as the other platforms. The runner's install is only used to
# unpack the MSI; the bundled copy is the pinned one, not whatever the runner happens to carry.
$sevenDir = "C:\Program Files\7-Zip"
$sevenExe = Join-Path $sevenDir "7z.exe"
if (Test-Path $sevenExe) {
  $msi = Get-Pinned `
    "https://github.com/ip7z/7zip/releases/download/26.03/7z2603-x64.msi" `
    "C0680064D698A62DD4A5A47F403DB356A6531A5473E4C4B1D090EA2590513926" `
    "7z2603-x64.msi"
  $zdir = Join-Path $tmp "7zmsi"
  & $sevenExe x $msi "-o$zdir" -y | Out-Null
  Copy-Item (Join-Path $zdir "_7z.exe") (Join-Path $dst "7z.exe") -Force
  Copy-Item (Join-Path $zdir "_7z.dll") (Join-Path $dst "7z.dll") -Force
} else {
  Write-Warning "7-Zip not found; archives will rely on a system install"
}

Get-ChildItem $dst -Recurse -Include *.exe | ForEach-Object { $_.FullName }
