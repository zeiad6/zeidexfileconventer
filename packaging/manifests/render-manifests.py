#!/usr/bin/env python3
"""Render or validate Winget, Scoop, and Homebrew package manager manifests."""

from __future__ import annotations

import argparse
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
MANIFEST_DIR = pathlib.Path(__file__).resolve().parent


def read_cargo_version() -> str:
    cargo = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    for line in cargo.splitlines():
        line = line.strip()
        if line.startswith("version ="):
            return line.split("=", 1)[1].strip().strip('"')
    raise RuntimeError("Could not find workspace version in Cargo.toml")


def check_templates() -> None:
    required = [
        MANIFEST_DIR / "winget" / "Zeidexsoft.ZeidexConvert.yaml",
        MANIFEST_DIR / "winget" / "Zeidexsoft.ZeidexConvert.installer.yaml",
        MANIFEST_DIR / "winget" / "Zeidexsoft.ZeidexConvert.locale.en-US.yaml",
        MANIFEST_DIR / "scoop" / "zeidex-convert.json",
        MANIFEST_DIR / "homebrew" / "zeidex-convert.rb",
    ]
    for path in required:
        if not path.is_file():
            raise SystemExit(f"missing manifest template: {path}")
        text = path.read_text(encoding="utf-8")
        if "__VERSION__" not in text:
            raise SystemExit(f"template missing __VERSION__ placeholder: {path}")
    scoop_raw = (MANIFEST_DIR / "scoop" / "zeidex-convert.json").read_text(encoding="utf-8")
    json.loads(scoop_raw)
    print(f"All {len(required)} package manager manifest templates validated.")


def render(out_dir: pathlib.Path, version: str, win_setup_sha: str, win_cli_sha: str, mac_arm_sha: str, mac_x64_sha: str) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    replacements = {
        "__VERSION__": version,
        "__WIN_SETUP_SHA256__": win_setup_sha.upper(),
        "__WIN_CLI_SHA256__": win_cli_sha.lower(),
        "__MAC_ARM64_SHA256__": mac_arm_sha.lower(),
        "__MAC_X64_SHA256__": mac_x64_sha.lower(),
    }
    for src in MANIFEST_DIR.rglob("*"):
        if not src.is_file() or src.name == pathlib.Path(__file__).name:
            continue
        rel = src.relative_to(MANIFEST_DIR)
        dst = out_dir / rel
        dst.parent.mkdir(parents=True, exist_ok=True)
        content = src.read_text(encoding="utf-8")
        for k, v in replacements.items():
            content = content.replace(k, v)
        dst.write_text(content, encoding="utf-8")
    print(f"Rendered manifests for v{version} into {out_dir}")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true", help="Validate manifest templates")
    ap.add_argument("--out", type=pathlib.Path, help="Output directory for rendered manifests")
    ap.add_argument("--version", default=read_cargo_version())
    ap.add_argument("--win-setup-sha256", default="0" * 64)
    ap.add_argument("--win-cli-sha256", default="0" * 64)
    ap.add_argument("--mac-arm64-sha256", default="0" * 64)
    ap.add_argument("--mac-x64-sha256", default="0" * 64)
    args = ap.parse_args()

    if args.check:
        check_templates()
        return 0
    if args.out is None:
        ap.error("--out is required unless --check is passed")
    render(
        args.out,
        args.version,
        args.win_setup_sha256,
        args.win_cli_sha256,
        args.mac_arm64_sha256,
        args.mac_x64_sha256,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
