#!/usr/bin/env python3
"""Render every install icon from `app/src-tauri/icons/icon.svg`.

One source, one command:

    python3 packaging/make-icons.py

The SVG is the artwork; this script only decides sizes and containers. Every output is a
straight re-render of the vector at its final pixel size (never a downscale of a large raster),
which is what keeps the 16 px copy crisp — the ring is 1.4 px thick there, and a resampled
version blurs it into a smudge.

Containers:

  PNG   every size tauri, a Linux .desktop file and the Windows Store logos ask for.
  ICO   a hand-written directory of PNG entries. Pillow's writer only resizes one bitmap, so
        the small sizes would come out soft; entries here are rendered at their own size.
  ICNS  the same, with the artwork scaled onto Apple's 824/1024 icon grid. macOS does not crop
        app icons, so an icon that is too large looks oversized beside its neighbours in the
        dock; the PNG and ICO outputs stay at the 896/1024 Windows and Linux expect.

Run it after any edit to icon.svg and commit the results: the installer reads the PNG/ICO/ICNS
files, never the SVG.
"""

from __future__ import annotations

import shutil
import struct
import subprocess
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
ICONS = ROOT / "app" / "src-tauri" / "icons"
SOURCE = ICONS / "icon.svg"
#: Optical variant for the sizes where the main artwork aliases away. Above this the full icon is
#: sharper; at or below it the simplified one is.
SMALL = ICONS / "icon-small.svg"
SMALL_MAX = 24
WORK = ROOT / "target" / "icons-build"

#: Windows Store / MSIX tiles, all rendered from the same artwork.
SQUARE_LOGOS = {
    "Square30x30Logo.png": 30,
    "Square44x44Logo.png": 44,
    "Square71x71Logo.png": 71,
    "Square89x89Logo.png": 89,
    "Square107x107Logo.png": 107,
    "Square142x142Logo.png": 142,
    "Square150x150Logo.png": 150,
    "Square284x284Logo.png": 284,
    "Square310x310Logo.png": 310,
    "StoreLogo.png": 50,
}

#: Plain PNGs: tauri's bundle.icon list plus the window icon.
PNGS = {
    "icon.png": 1024,
    "512x512.png": 512,
    "128x128@2x.png": 256,
    "128x128.png": 128,
    "64x64.png": 64,
    "32x32.png": 32,
}

ICO_SIZES = (16, 24, 32, 48, 64, 128, 256)

#: ICNS entry type -> pixel size. The @2x members (ic11..ic14) duplicate a size already listed;
#: both are written because older macOS rereads the @1x slot and newer the @2x one.
ICNS = (
    ("icp4", 16),
    ("icp5", 32),
    ("icp6", 64),
    ("ic07", 128),
    ("ic08", 256),
    ("ic09", 512),
    ("ic10", 1024),
    ("ic11", 32),
    ("ic12", 64),
    ("ic13", 256),
    ("ic14", 512),
)

#: Apple's icon grid: the rounded square occupies 824 of 1024 with 100 px of breathing room.
#: The source squircle is 896 wide inside the 1024 canvas, so the artwork is scaled by 824/896 —
#: scaling the whole canvas would leave the shape at 720 and the icon visibly undersized in the
#: dock beside native apps.
MACOS_CONTENT = 824 / 896


def render(size: int, *, content: float = 1.0) -> Image.Image:
    """Render the artwork at `size` px. `content` shrinks it onto a transparent canvas.

    Sizes at or below `SMALL_MAX` come from `icon-small.svg`: at 16 px the shadow and the halo of
    the full artwork are grey haze, and the ring is thinner than one pixel.
    """
    if size < 1:
        raise ValueError(f"icon size must be positive, got {size}")
    source = SMALL if size <= SMALL_MAX else SOURCE
    if content == 1.0:
        target = size
    else:
        target = max(1, round(size * content))
    out = WORK / f"render-{source.stem}-{target}.png"
    subprocess.run(
        ["rsvg-convert", "-w", str(target), "-h", str(target), str(source), "-o", str(out)],
        check=True,
    )
    image = Image.open(out).convert("RGBA")
    if target == size:
        return image
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    offset = ((size - target) // 2, (size - target) // 2)
    canvas.paste(image, offset)
    return canvas


def write_png(image: Image.Image, path: Path) -> None:
    image.save(path, format="PNG", optimize=True)


def write_ico(path: Path, images: dict[int, Image.Image]) -> None:
    """A PNG-compressed ICO (Vista and later). Every entry keeps its own rendered bitmap."""
    blobs = []
    for size, image in images.items():
        scratch = WORK / f"ico-{size}.png"
        image.save(scratch, format="PNG", optimize=True)
        blobs.append((size, scratch.read_bytes()))

    header = struct.pack("<HHH", 0, 1, len(blobs))
    offset = 6 + 16 * len(blobs)
    directory = b"".join(
        struct.pack(
            "<BBBBHHII",
            0 if size >= 256 else size,  # 0 means 256 in the ICO directory
            0 if size >= 256 else size,
            0,  # palette entries: none, the images carry alpha
            0,
            1,  # colour planes
            32,  # bits per pixel
            len(data),
            offset + sum(len(d) for _, d in blobs[:i]),
        )
        for i, (size, data) in enumerate(blobs)
    )
    path.write_bytes(header + directory + b"".join(data for _, data in blobs))


def write_icns(path: Path, images: dict[int, Image.Image]) -> None:
    chunks = []
    for kind, size in ICNS:
        scratch = WORK / f"icns-{kind}.png"
        images[size].save(scratch, format="PNG", optimize=True)
        data = scratch.read_bytes()
        chunks.append(kind.encode("ascii") + struct.pack(">I", len(data) + 8) + data)
    body = b"".join(chunks)
    path.write_bytes(b"icns" + struct.pack(">I", len(body) + 8) + body)


#: NSIS asks for these two exact sizes; BMP has no alpha, so both are drawn on their own plate.
HEADER = (150, 57)
SIDEBAR = (164, 314)
DEJAVU = Path("/usr/share/fonts/truetype/dejavu")


def plate(size: tuple[int, int], top: str, bottom: str) -> Image.Image:
    """A vertical gradient. BMP has no alpha, so every pixel is written explicitly."""
    width, height = size
    image = Image.new("RGB", size)
    pixels = image.load()
    start = tuple(int(top[i:i + 2], 16) for i in (1, 3, 5))
    end = tuple(int(bottom[i:i + 2], 16) for i in (1, 3, 5))
    for y in range(height):
        t = y / max(1, height - 1)
        colour = tuple(round(start[i] + (end[i] - start[i]) * t) for i in range(3))
        for x in range(width):
            pixels[x, y] = colour
    return image


def sidebar() -> Image.Image:
    image = plate(SIDEBAR, "#1d1916", "#0f0c0a").convert("RGBA")
    icon = render(96)
    image.alpha_composite(icon, ((SIDEBAR[0] - 96) // 2, 34))

    bold = ImageFont.truetype(str(DEJAVU / "DejaVuSans-Bold.ttf"), 15)
    body = ImageFont.truetype(str(DEJAVU / "DejaVuSans.ttf"), 11)
    draw = ImageDraw.Draw(image)
    draw.text((SIDEBAR[0] // 2, 168), "Zeidex Convert", font=bold, fill="#f6f1ea", anchor="mt")
    for i, line in enumerate(("Convert, compress, OCR,", "and edit files locally.", "Nothing is uploaded.")):
        draw.text((SIDEBAR[0] // 2, 196 + i * 18), line, font=body, fill="#b4a99e", anchor="mt")
    # A short accent rule closes the panel; without it the copy floats in the dark.
    draw.rounded_rectangle((SIDEBAR[0] // 2 - 18, 264, SIDEBAR[0] // 2 + 18, 267), 2, fill="#f07b1d")
    return image.convert("RGB")


def header() -> Image.Image:
    image = plate(HEADER, "#191512", "#100d0b").convert("RGBA")
    icon = render(38)
    image.alpha_composite(icon, (10, (HEADER[1] - 38) // 2))

    draw = ImageDraw.Draw(image)
    bold = ImageFont.truetype(str(DEJAVU / "DejaVuSans-Bold.ttf"), 13)
    thin = ImageFont.truetype(str(DEJAVU / "DejaVuSans.ttf"), 10)
    draw.text((56, 16), "Zeidex", font=bold, fill="#f6f1ea", anchor="lt")
    draw.text((56, 33), "Convert", font=thin, fill="#ff9e4a", anchor="lt")
    return image.convert("RGB")


def write_installer_art() -> list[Path]:
    """NSIS header/sidebar. They are the first screen a Windows user sees, so they carry the
    same artwork as the icon instead of the previous stale lockup."""
    target = ROOT / "app" / "src-tauri" / "assets" / "installer"
    target.mkdir(parents=True, exist_ok=True)
    out = []
    for name, image in (("header.bmp", header()), ("sidebar.bmp", sidebar())):
        path = target / name
        image.save(path, format="BMP")
        out.append(path)
    return out


def main() -> int:
    if shutil.which("rsvg-convert") is None:
        print("rsvg-convert is required (librsvg2-bin on Debian/Ubuntu, librsvg on Homebrew)", file=sys.stderr)
        return 2
    for source in (SOURCE, SMALL):
        if not source.is_file():
            print(f"missing artwork: {source}", file=sys.stderr)
            return 2
    WORK.mkdir(parents=True, exist_ok=True)

    # The largest file first: every other render reuses the same code path, so a broken SVG fails
    # once, loudly, instead of producing a directory of half-rendered icons.
    big = render(1024)
    write_png(big, ICONS / "icon.png")

    for name, size in PNGS.items():
        write_png(big if size == 1024 else render(size), ICONS / name)

    for name, size in SQUARE_LOGOS.items():
        write_png(render(size), ICONS / name)

    # The About dialog and the home rail load their logo from the web root, so that copy has to
    # carry the same artwork; 256 px keeps it sharp on a 2x display at 42 px.
    shutil.copyfile(ICONS / "128x128@2x.png", ROOT / "app" / "public" / "icon.png")

    write_ico(ICONS / "icon.ico", {size: render(size) for size in ICO_SIZES})

    macos = {size: render(size, content=MACOS_CONTENT) for _, size in ICNS}
    write_icns(ICONS / "icon.icns", macos)

    for path in write_installer_art():
        print(f"  {path.relative_to(ROOT)}")

    written = sorted(p.name for p in ICONS.iterdir() if p.suffix in {".png", ".ico", ".icns"})
    print(f"{len(written)} icons written to {ICONS.relative_to(ROOT)}")
    for name in written:
        print(f"  {name}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
