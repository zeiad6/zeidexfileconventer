#!/usr/bin/env python3
"""Copies a few programs out of a conda environment together with only the shared libraries
they load, as a tree with no symbolic links.

Why: a conda environment that holds libvips weighs over a gigabyte (ImageMagick, GTK, Graphviz,
every Tesseract language), and Tauri's resource copier follows each symlink into a second full
copy of the library. The package needs a few programs and their closure, once.

usage: slim-conda.py <env> <out> <program-or-relative-path>...

Programs are names under <env>/bin (or <env>/Library/bin on Windows). Any other argument is a
path relative to <env> (file or directory) copied as is, and scanned for libraries too.
Library names are read straight from the bytes of each binary, which works for ELF, Mach-O and
PE alike and needs no readelf/otool on the build machine.
"""

import os
import re
import shutil
import sys

LIB_NAME = re.compile(rb"[A-Za-z0-9_+.-]+\.(?:so(?:\.[0-9]+)*|dylib|dll)\b", re.IGNORECASE)


def main() -> None:
    env, out, wanted = sys.argv[1], sys.argv[2], sys.argv[3:]
    windows = os.path.isdir(os.path.join(env, "Library", "bin"))
    bin_rel = os.path.join("Library", "bin") if windows else "bin"
    lib_rel = bin_rel if windows else "lib"
    lib_dir = os.path.join(env, lib_rel)
    # Lowercased on Windows only: its loader ignores case, the others do not.
    key = (lambda n: n.lower()) if windows else (lambda n: n)
    libs = {key(n): n for n in os.listdir(lib_dir)}

    queue: list[str] = []  # paths relative to env
    for item in wanted:
        if os.sep in item or "/" in item:
            queue.append(item)
            continue
        names = [item + ".exe", item + ".bat", item] if windows else [item]
        hit = next((n for n in names if os.path.isfile(os.path.join(env, bin_rel, n))), None)
        if hit is None:
            sys.exit(f"{item} is not in {os.path.join(env, bin_rel)}")
        queue.append(os.path.join(bin_rel, hit))

    seen: set[str] = set()
    while queue:
        rel = queue.pop()
        if rel in seen:
            continue
        seen.add(rel)
        src = os.path.join(env, rel)
        if os.path.isdir(src):
            for name in os.listdir(src):
                queue.append(os.path.join(rel, name))
            continue
        if not os.path.isfile(src):  # dangling link
            continue
        dst = os.path.join(out, rel)
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        shutil.copy2(src, dst)  # follows the link: the needed name becomes a regular file
        with open(src, "rb") as fh:
            data = fh.read()
        if data[:4] not in (b"\x7fELF", b"\xcf\xfa\xed\xfe", b"\xca\xfe\xba\xbe") and data[:2] != b"MZ":
            continue
        for match in LIB_NAME.findall(data):
            name = libs.get(key(match.decode("ascii", "replace")))
            if name:
                queue.append(os.path.join(lib_rel, name))

    size = sum(os.path.getsize(os.path.join(d, f)) for d, _, fs in os.walk(out) for f in fs)
    print(f"{out}: {len(seen)} entries, {size // (1 << 20)} MiB")


if __name__ == "__main__":
    main()
