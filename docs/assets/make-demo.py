#!/usr/bin/env python3
"""Renders the README demo clip: four SVG panels -> PNG -> MP4 + GIF.

Text is laid out by Pango through rsvg-convert, so Arabic is shaped correctly; ffmpeg only
crossfades the finished frames. Run from the repository root:

    python3 docs/assets/make-demo.py
"""
from __future__ import annotations

import pathlib
import shutil
import subprocess

W, H = 960, 540
OUT = pathlib.Path(__file__).resolve().parent
SLIDES = OUT / "demo"
FPS = 30
HOLD = 3.4  # seconds per panel
FADE = 0.7

BASE = """<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">
  <defs>
    <linearGradient id="ink" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="#161412"/><stop offset="0.6" stop-color="#1d1916"/><stop offset="1" stop-color="#2a1d12"/>
    </linearGradient>
    <radialGradient id="glow" cx="0.82" cy="0.18" r="0.6">
      <stop offset="0" stop-color="#f07b1d" stop-opacity="0.42"/>
      <stop offset="1" stop-color="#f07b1d" stop-opacity="0"/>
    </radialGradient>
    <pattern id="grid" width="32" height="32" patternUnits="userSpaceOnUse">
      <path d="M32 0H0V32" fill="none" stroke="#ffffff" stroke-opacity="0.04" stroke-width="1"/>
    </pattern>
    <style>
      .latin {{ font-family: 'IBM Plex Sans', sans-serif; }}
      .ar {{ font-family: 'IBM Plex Sans Arabic', 'Noto Sans Arabic UI', sans-serif; }}
      .kicker {{ font-size: 15px; font-weight: 600; fill: #f0a35f; letter-spacing: 2.4px; }}
      .title {{ font-size: 54px; font-weight: 700; fill: #f7f3ee; letter-spacing: -1px; }}
      .ar-big {{ font-size: 35px; font-weight: 600; fill: #f7f3ee; }}
      .ar-mid {{ font-size: 26px; font-weight: 500; fill: #efe9e2; }}
      .muted {{ font-size: 17px; fill: #a9a095; }}
      .chip {{ font-size: 16px; font-weight: 600; fill: #f5efe7; }}
    </style>
  </defs>
  <rect width="{w}" height="{h}" fill="url(#ink)"/>
  <rect width="{w}" height="{h}" fill="url(#grid)"/>
  <ellipse cx="{gx}" cy="90" rx="360" ry="220" fill="url(#glow)"/>
  {body}
</svg>
"""


def chip(x: int, y: int, label: str, accent: bool = False) -> str:
    fill = "#f07b1d" if accent else "rgba(255,255,255,0.07)"
    stroke = "#f07b1d" if accent else "rgba(255,255,255,0.12)"
    colour = "#ffffff" if accent else "#ffb066"
    return (
        f'<g transform="translate({x} {y})">'
        f'<rect x="-46" y="-19" width="92" height="38" rx="19" fill="{fill}" stroke="{stroke}"/>'
        f'<text class="latin chip" x="0" y="6" text-anchor="middle" fill="{colour}">{label}</text></g>'
    )


def slide_one() -> str:
    body = (
        '<text class="latin kicker" x="64" y="120">ZEIDEX · LOCAL FILE CONVERTER</text>'
        '<text class="latin title" x="64" y="200">Zeidex Convert</text>'
        '<text class="ar ar-big" x="64" y="286">حوّل أي ملف، على جهازك.</text>'
        '<text class="latin muted" x="64" y="330">No uploads. No accounts. No telemetry.</text>'
        + chip(760, 120, "MP4", True) + chip(852, 200, "PDF") + chip(770, 280, "DOCX")
        + chip(600, 360, "SRT") + chip(704, 420, "HEIC")
        + '<rect x="64" y="392" width="196" height="44" rx="22" fill="#f07b1d"/>'
        '<text class="latin" x="162" y="420" text-anchor="middle" font-size="18" font-weight="600" fill="#fff">100% offline</text>'
    )
    return BASE.format(w=W, h=H, gx=820, body=body)


def slide_two() -> str:
    body = (
        '<text class="latin kicker" x="64" y="96">THE WHEEL</text>'
        '<text class="ar ar-mid" x="64" y="164">اسحب الملف، اضغط Shift، أفلته على الصيغة.</text>'
        '<circle cx="480" cy="352" r="118" fill="none" stroke="#f07b1d" stroke-opacity="0.35" stroke-width="2"/>'
        '<circle cx="480" cy="352" r="82" fill="#ffffff"/>'
        '<circle cx="480" cy="352" r="58" fill="#fdecdf"/>'
        '<path d="M466 322h18l16 16v40a6 6 0 0 1-6 6h-34a6 6 0 0 1-6-6v-50a6 6 0 0 1 6-6z" fill="#fff" stroke="#f07b1d" stroke-width="3"/>'
        '<path d="M484 322v12a5 5 0 0 0 5 5h11" fill="none" stroke="#f07b1d" stroke-width="3"/>'
        '<g transform="translate(480 418)"><rect x="-58" y="-16" width="116" height="32" rx="9" fill="#1d1b19"/>'
        '<text class="latin" x="0" y="6" text-anchor="middle" font-size="14" font-weight="600" fill="#f7f5f2">hold ⇧ Shift</text></g>'
        + chip(480, 214, "MP4", True) + chip(604, 262, "WEBM") + chip(640, 352, "GIF")
        + chip(600, 444, "MP3") + chip(480, 492, "SRT") + chip(356, 444, "MOV")
        + chip(320, 352, "HEIC") + chip(356, 262, "PDF")
    )
    return BASE.format(w=W, h=H, gx=760, body=body)


def slide_three() -> str:
    rows = [
        ("Nothing leaves the machine", "لا يغادر أي ملف جهازك"),
        ("No sign-up, no cloud, no queue", "بلا حساب، بلا سحابة، بلا انتظار"),
        ("SHA-256 on every download", "بصمة SHA-256 لكل تنزيل"),
    ]
    items = "".join(
        f'<g transform="translate(96 {252 + i * 62})">'
        f'<circle cx="12" cy="-6" r="11" fill="#2f8f4e"/>'
        f'<path d="M6 -6l4 4 8-8" fill="none" stroke="#fff" stroke-width="2.4" stroke-linecap="round"/>'
        f'<text class="latin" x="38" y="0" font-size="19" fill="#f2eee9">{en}</text>'
        f'<text class="ar" x="38" y="26" font-size="16" fill="#a9a095">{ar}</text></g>'
        for i, (en, ar) in enumerate(rows)
    )
    body = (
        '<text class="latin kicker" x="64" y="96">PRIVACY BY CONSTRUCTION</text>'
        '<text class="ar ar-big" x="64" y="168">بياناتك تبقى عندك.</text>'
        + items
        + '<g transform="translate(732 132)"><path d="M0 0h84a10 10 0 0 1 10 10v62c0 44-32 74-52 84-20-10-52-40-52-84V10a10 10 0 0 1 10-10z" fill="rgba(240,123,29,0.12)" stroke="#f07b1d" stroke-opacity="0.5" stroke-width="2"/><path d="M22 70l16 16 30-34" fill="none" stroke="#f07b1d" stroke-width="6" stroke-linecap="round"/></g>'
        '<text class="latin muted" x="64" y="480">Works without a network connection, forever.</text>'
    )
    return BASE.format(w=W, h=H, gx=800, body=body)


def slide_four() -> str:
    body = (
        '<text class="latin kicker" x="64" y="96">ONE RELEASE, EVERY SYSTEM</text>'
        '<text class="latin title" x="64" y="164">Install it anywhere.</text>'
        '<text class="ar ar-mid" x="64" y="222">حزمة تثبيت لكل نظام، وتغطية كاملة لتوزيعات لينكس.</text>'
        + chip(120, 300, "Windows") + chip(268, 300, "macOS") + chip(406, 300, "Linux", True) + chip(534, 300, "Android")
        + chip(120, 366, "MSI") + chip(224, 366, "NSIS") + chip(330, 366, "deb") + chip(420, 366, "rpm")
        + chip(520, 366, "AppImage") + chip(654, 366, "Flatpak")
        + '<rect x="64" y="424" width="832" height="56" rx="12" fill="rgba(255,255,255,0.05)" stroke="rgba(255,255,255,0.1)"/>'
        '<text class="latin" x="88" y="458" font-size="15" fill="#ffb066">curl -fsSL https://raw.githubusercontent.com/zeiad6/zeidexfileconventer/main/packaging/linux/install.sh | sh</text>'
    )
    return BASE.format(w=W, h=H, gx=720, body=body)


def render() -> list[pathlib.Path]:
    SLIDES.mkdir(parents=True, exist_ok=True)
    frames = []
    for index, body in enumerate((slide_one(), slide_two(), slide_three(), slide_four()), start=1):
        svg = SLIDES / f"slide-{index}.svg"
        png = SLIDES / f"slide-{index}.png"
        svg.write_text(body, encoding="utf-8")
        subprocess.run(["rsvg-convert", "-w", str(W), "-h", str(H), str(svg), "-o", str(png)], check=True)
        frames.append(png)
    return frames


def assemble(frames: list[pathlib.Path]) -> None:
    inputs: list[str] = []
    for frame in frames:
        inputs += ["-loop", "1", "-t", str(HOLD), "-i", str(frame)]
    filters = []
    last = "0:v"
    offset = HOLD - FADE
    for index in range(1, len(frames)):
        out = f"v{index}"
        filters.append(
            f"[{last}][{index}:v]xfade=transition=fade:duration={FADE}:offset={offset:.2f}[{out}]"
        )
        last = out
        offset += HOLD - FADE
    graph = ";".join(filters) + f";[{last}]format=yuv420p[v]"
    mp4 = OUT / "demo.mp4"
    subprocess.run(
        ["ffmpeg", "-y", *inputs, "-filter_complex", graph, "-map", "[v]",
         "-c:v", "libx264", "-preset", "slow", "-crf", "20", "-movflags", "+faststart", str(mp4)],
        check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    gif = OUT / "demo.gif"
    subprocess.run(
        ["ffmpeg", "-y", "-i", str(mp4), "-vf",
         f"fps=12,scale=720:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=128[p];[b][p]paletteuse=dither=bayer",
         "-loop", "0", str(gif)],
        check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    for path in (mp4, gif):
        print(f"{path.name}: {path.stat().st_size / 1_048_576:.2f} MB")
    shutil.rmtree(SLIDES)


if __name__ == "__main__":
    assemble(render())
