# Downloads the redistributable engines bundled into the Windows build (CI and local).
# FFmpeg (GPL build, BtbN), libvips (LGPL), qpdf (Apache-2.0), 7-Zip (LGPL, from the runner's install).
# Ghostscript (AGPL), LibreOffice, Pandoc, Calibre and Tesseract are NOT bundled: the app's
# Engines panel shows the winget command for each one.
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

# FFmpeg + FFprobe
$zip = Get-Asset "BtbN/FFmpeg-Builds" '^ffmpeg-master-latest-win64-gpl\.zip$'
Expand-Archive $zip (Join-Path $tmp "ffmpeg") -Force
Get-ChildItem (Join-Path $tmp "ffmpeg") -Recurse -Include ffmpeg.exe, ffprobe.exe | Copy-Item -Destination $dst

# libvips: keep its bin/ folder intact, vips.exe needs the DLLs beside it.
$zip = Get-Asset "libvips/build-win64-mxe" '^vips-dev-x64-all-[\d.]+\.zip$'
Expand-Archive $zip (Join-Path $tmp "vips") -Force
$bin = Get-ChildItem (Join-Path $tmp "vips") -Recurse -Directory -Filter bin | Select-Object -First 1
New-Item -ItemType Directory -Force (Join-Path $dst "vips") | Out-Null
Copy-Item $bin.FullName (Join-Path $dst "vips") -Recurse -Force

# qpdf
$zip = Get-Asset "qpdf/qpdf" '^qpdf-[\d.]+-msvc64\.zip$'
Expand-Archive $zip (Join-Path $tmp "qpdf") -Force
$bin = Get-ChildItem (Join-Path $tmp "qpdf") -Recurse -Directory -Filter bin | Select-Object -First 1
New-Item -ItemType Directory -Force (Join-Path $dst "qpdf") | Out-Null
Copy-Item $bin.FullName (Join-Path $dst "qpdf") -Recurse -Force

# 7-Zip ships preinstalled on GitHub's Windows runners.
$seven = "C:\Program Files\7-Zip"
if (Test-Path "$seven\7z.exe") { Copy-Item "$seven\7z.exe", "$seven\7z.dll" $dst }
else { Write-Warning "7-Zip not found; archives will rely on a system install" }

Get-ChildItem $dst -Recurse -Include *.exe | ForEach-Object { $_.FullName }
