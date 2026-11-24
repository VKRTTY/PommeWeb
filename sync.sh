#!/usr/bin/env bash
# Copies the web port's Rust side out of the Pomme checkout: new browser-only
# files into rust/ (at their Pomme paths), and the port's edits to Pomme's
# own files into patches/pomme.patch.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="${POMME_REPO:-$HOME/rust mc}"

cd "$REPO"
rm -rf "$HERE/rust"
git ls-files --others --exclude-standard | grep -v '^third_party/simdnbt/' | while read -r file; do
  mkdir -p "$HERE/rust/$(dirname "$file")"
  cp "$file" "$HERE/rust/$file"
done

mkdir -p "$HERE/patches"
{
  git diff HEAD --binary
  git ls-files --others --exclude-standard -- third_party/simdnbt | while read -r file; do
    git diff --binary --no-index /dev/null "$file" || true
  done
} > "$HERE/patches/pomme.patch"
git rev-parse HEAD > "$HERE/patches/BASE"
echo "Synced from $REPO at $(cat "$HERE/patches/BASE")"
