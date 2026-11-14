#!/usr/bin/env python3
"""Rewrite the desktop and CLI rows of packaging/updates/manifest.json from a GitHub release.

The Android row is left alone: the APK is published by the Android repository's own workflow and
its digest is pinned there. Every other row is regenerated from the assets that actually exist on
the release, so the registry can never describe a file that was never uploaded or carry a digest
from an older upload.

Usage: packaging/sync-release-digests.py <tag>   e.g. v0.3.2
Requires: gh, authenticated. Run after the release workflow finishes uploading so every row
describes a file that exists.
"""
from __future__ import annotations

import json
import pathlib
import subprocess
import sys
from datetime import date

ROOT = pathlib.Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "packaging" / "updates" / "manifest.json"
if len(sys.argv) != 2:
    print("usage: packaging/sync-release-digests.py <tag>", file=sys.stderr)
    raise SystemExit(2)
TAG = sys.argv[1]
CHANGELOG_AR = (
    "إصدار {version}: حزمة واحدة لكل نظام — Windows (setup)، Linux (AppImage لـ x64 وarm64)، "
    "macOS (Apple Silicon وIntel)، وأندرويد (APK). كل حزمة سطح مكتب تحمل محرّكاتها كاملة: "
    "FFmpeg و7-Zip وPandoc وrhwp وqpdf وGhostscript، فلا حاجة لتثبيت أي شيء آخر."
)
CHANGELOG_EN = (
    "Release {version}: one package per system — Windows setup, Linux AppImage (x64 and arm64), "
    "macOS DMG (Apple Silicon and Intel) and the Android APK. Every desktop package carries its "
    "engines — FFmpeg, 7-Zip, Pandoc, rhwp, qpdf and Ghostscript — so nothing else needs installing."
)


def release(tag: str) -> dict:
    # `gh release view --json` returns neither browser_download_url nor digest, so the REST
    # payload is used instead: it carries both for every asset.
    out = subprocess.check_output(
        ["gh", "api", f"repos/{repo()}/releases/tags/{tag}"],
        text=True,
    )
    return json.loads(out)


def repo() -> str:
    return subprocess.check_output(
        ["gh", "repo", "view", "--json", "nameWithOwner", "--jq", ".nameWithOwner"], text=True
    ).strip()


def classify(name: str) -> tuple[str, str, str] | None:
    """Only the assets the release workflow actually uploads: one package per system."""
    lower = name.lower()
    if lower.endswith("-setup.exe"):
        return "zeidex-convert-windows-x64-setup", "windows", "x64"
    if lower.endswith(".appimage"):
        arch = "arm64" if ("aarch64" in lower or "arm64" in lower) else "amd64"
        return f"zeidex-convert-linux-{arch}-appimage", "linux", arch
    if lower.endswith(".dmg"):
        arch = "arm64" if "aarch64" in lower else "x64"
        return f"zeidex-convert-macos-{arch}", "macos", arch
    return None


def version_code(version: str) -> int:
    major, minor, patch = (int(part) for part in version.split("."))
    return major * 10_000 + minor * 100 + patch


def main() -> int:
    version = TAG.lstrip("v")
    data = json.loads(MANIFEST.read_text())
    android = [p for p in data["packages"] if p.get("platform") == "android"]
    release_data = release(TAG)

    rows: list[dict] = list(android)
    missing: list[str] = []
    for asset in sorted(release_data["assets"], key=lambda a: a["name"]):
        kind = classify(asset["name"])
        if kind is None:
            continue
        package_id, platform, arch = kind
        digest = (asset.get("digest") or "").removeprefix("sha256:")
        if len(digest) != 64:
            missing.append(asset["name"])
            continue
        rows.append(
            {
                "id": package_id,
                "name": f"Zeidex Convert ({asset['name']})",
                "platform": platform,
                "arch": arch,
                "version": version,
                "version_code": version_code(version),
                "release_date": release_data["published_at"][:10],
                "url": asset["browser_download_url"],
                "mirror_url": asset["browser_download_url"],
                "sha256": digest,
                "changelog_ar": CHANGELOG_AR.format(version=version),
                "changelog_en": CHANGELOG_EN.format(version=version),
                "mandatory": False,
            }
        )

    if missing:
        print("assets without a digest, not written:", ", ".join(missing), file=sys.stderr)
        return 1

    data["updated_at"] = f"{date.today().isoformat()}T00:00:00Z"
    data["packages"] = rows
    MANIFEST.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n")
    print(f"wrote {len(rows)} rows for {TAG}: " + ", ".join(r["id"] for r in rows))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
