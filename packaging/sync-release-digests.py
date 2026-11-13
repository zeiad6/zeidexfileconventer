#!/usr/bin/env python3
"""Rewrite the desktop and CLI rows of packaging/updates/manifest.json from a GitHub release.

The Android row is left alone: the APK is published by the Android repository's own workflow and
its digest is pinned there. Every other row is regenerated from the assets that actually exist on
the release, so the registry can never describe a file that was never uploaded or carry a digest
from an older upload.

Usage: packaging/sync-release-digests.py [tag]   (default: v0.2.0)
Requires: gh, authenticated.
"""
from __future__ import annotations

import json
import pathlib
import re
import subprocess
import sys
from datetime import date

ROOT = pathlib.Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "packaging" / "updates" / "manifest.json"
TAG = sys.argv[1] if len(sys.argv) > 1 else "v0.2.0"
CHANGELOG_AR = (
    "إصدار {version}: حزم تثبيت لكل الأنظمة — Windows (MSI وsetup)، Linux (AppImage وdeb وrpm "
    "وFlatpak)، macOS (Apple Silicon وIntel)، وسطر أوامر لكل نظام."
)
CHANGELOG_EN = (
    "Release {version}: installers for every system — Windows (MSI and setup), Linux (AppImage, "
    "deb, rpm and Flatpak), macOS (Apple Silicon and Intel), and the CLI for each platform."
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
    if name.endswith(".msi"):
        return "zeidex-convert-windows-x64-msi", "windows", "x64"
    if name.endswith("-setup.exe"):
        return "zeidex-convert-windows-x64-setup", "windows", "x64"
    if name.endswith(".deb"):
        return "zeidex-convert-linux-amd64-deb", "linux", "amd64"
    if name.endswith(".rpm"):
        return "zeidex-convert-linux-amd64-rpm", "linux", "amd64"
    if name.endswith(".AppImage"):
        return "zeidex-convert-linux-amd64-appimage", "linux", "amd64"
    if name.endswith(".flatpak"):
        return "com.zeidexsoft.convert.flatpak", "linux", "x86_64"
    if name.endswith(".dmg"):
        arch = "arm64" if "aarch64" in name else "x64"
        return f"zeidex-convert-macos-{arch}", "macos", arch
    match = re.search(r"zeidex-convert-[\d.]+-(.+?)\.(tar\.gz|zip)$", name)
    if match:
        return f"zeidex-convert-cli-{match.group(1)}", "cli", match.group(1)
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
