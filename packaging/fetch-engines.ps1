# Downloads the redistributable engines bundled into the Windows build (CI and local).
# FFmpeg (GPL build, BtbN), libvips (LGPL), qpdf (Apache-2.0), Ghostscript (AGPL, conda-forge)
# and 7-Zip (LGPL, from the runner's install).
# LibreOffice, Pandoc, Calibre and Tesseract are NOT bundled: the app's Engines panel shows the
# winget command for each one.
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

# 7-Zip ships preinstalled on GitHub's Windows runners.
$sevenDir = "C:\Program Files\7-Zip"
if (Test-Path "$sevenDir\7z.exe") { Copy-Item "$sevenDir\7z.exe", "$sevenDir\7z.dll" $dst }
else { Write-Warning "7-Zip not found; archives will rely on a system install" }

Get-ChildItem $dst -Recurse -Include *.exe | ForEach-Object { $_.FullName }
