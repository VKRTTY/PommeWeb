#!/usr/bin/env bash
# Builds Pomme's browser client from the game repo into ./dist.
# The game code (pomme-client and its crates) stays in the repo; this folder
# holds only what is web-specific: the browser JavaScript and the page.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="${POMME_REPO:-$HOME/rust mc}"
PROFILE="${PROFILE:-release}"
TOOLCHAIN="$(sed -n 's/^channel *= *"\(.*\)"/\1/p' "$REPO/rust-toolchain.toml")"

# The repo pins a rustup nightly; a Homebrew cargo earlier on PATH has no
# wasm32 standard library.
TOOLCHAIN_DIR="$HOME/.rustup/toolchains/$TOOLCHAIN-$(rustc -vV | sed -n 's/^host: //p')"
export PATH="$TOOLCHAIN_DIR/bin:$PATH"
# This nightly's rust-lld looks for libLLVM beside itself rather than in the
# toolchain's lib/.
if [ "$(uname -s)" = Darwin ]; then
  export DYLD_FALLBACK_LIBRARY_PATH="$TOOLCHAIN_DIR/lib"
fi

cd "$REPO"
flag=""
[ "$PROFILE" = release ] && flag="--release"
cargo rustc -p pomme-client --lib --target wasm32-unknown-unknown --no-default-features \
  $flag --crate-type cdylib

DIST="$HERE/dist"
rm -rf "$DIST"
mkdir -p "$DIST"
BINDGEN="$HERE/tools/wasm-bindgen"
[ -x "$BINDGEN" ] || BINDGEN="$(command -v wasm-bindgen)" || {
  echo "Install wasm-bindgen 0.2.121: cargo install wasm-bindgen-cli --version 0.2.121" >&2; exit 1; }
"$BINDGEN" --target web --no-typescript --out-dir "$DIST" --out-name pomme \
  "$REPO/target/wasm32-unknown-unknown/$PROFILE/pomme_client.wasm"
cp "$HERE"/web/*.mjs "$HERE"/web/*.js "$HERE"/web/index.html "$DIST"/
date +%s > "$DIST/build.txt"
rm -f "$DIST"/*.test.mjs
echo "Built $DIST"
