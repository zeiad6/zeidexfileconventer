#!/usr/bin/env bash
# Print SHA-256 lines for release files, named the way GitHub serves them.
#
# GitHub rewrites spaces in release asset names to dots: a DMG built as
# "Zeidex Convert_0.3.2_aarch64.dmg" is published and downloaded as
# "Zeidex.Convert_0.3.2_aarch64.dmg". A checksum line copied straight from the
# build tree therefore names a file nobody has, and `sha256sum -c` fails on a
# good download. Hashing here, with the same rewrite, keeps the two in step.
#
# Usage: packaging/checksums.sh FILE... > SHA256SUMS-<platform>.txt
set -euo pipefail

if [ "$#" -eq 0 ]; then
  echo "usage: $0 FILE..." >&2
  exit 2
fi

if command -v sha256sum >/dev/null 2>&1; then
  hash_cmd=(sha256sum)
else
  hash_cmd=(shasum -a 256) # macOS
fi

for file in "$@"; do
  if [ ! -f "$file" ]; then
    echo "$0: no such file: $file" >&2
    exit 1
  fi
  name=$(basename "$file")
  name=${name// /.}
  digest=$("${hash_cmd[@]}" "$file" | cut -d' ' -f1)
  printf '%s  %s\n' "$digest" "$name"
done
