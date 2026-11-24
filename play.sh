#!/usr/bin/env bash
# One command to play: builds the page if it isn't built, starts the relay
# (and a local offline SteelMC server if one is built), and opens Chrome.
#   ./play.sh            play (builds only when dist/ is missing)
#   ./play.sh --build    rebuild first
# Then: Multiplayer -> Direct Connection -> 127.0.0.1 -> Join Server.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="${POMME_REPO:-$HOME/rust mc}"
PORT="${PORT:-8460}"
TOOLCHAIN="$(sed -n 's/^channel *= *"\(.*\)"/\1/p' "$REPO/rust-toolchain.toml")"
TOOLCHAIN_DIR="$HOME/.rustup/toolchains/$TOOLCHAIN-$(uname -m | sed 's/arm64/aarch64/')-apple-darwin"
export PATH="$TOOLCHAIN_DIR/bin:$PATH"
export DYLD_FALLBACK_LIBRARY_PATH="$TOOLCHAIN_DIR/lib"

if [ "${1:-}" = --build ] || [ ! -f "$HERE/dist/pomme_bg.wasm" ]; then
  "$HERE/build.sh"
fi

RELAY="$REPO/target/release/pomme-relay"
[ -x "$RELAY" ] || (cd "$REPO" && cargo build -p pomme-relay --release)

pids=()
trap 'kill "${pids[@]}" 2>/dev/null || true' EXIT

# A local offline server to join, when SteelMC has been built
# (cd "$REPO/third_party/SteelMC" && cargo build --release -p steel).
STEEL="$REPO/third_party/SteelMC/target/release/steel"
if [ -x "$STEEL" ] && ! nc -z 127.0.0.1 25565 2>/dev/null; then
  SERVER="$HERE/.server"
  mkdir -p "$SERVER/config"
  if [ ! -f "$SERVER/config/config.toml" ]; then
    sed -e 's/^online_mode = true/online_mode = false/' -e 's/^encryption = true/encryption = false/' \
      -e 's/^view_distance = .*/view_distance = 8/' -e 's/^simulation_distance = .*/simulation_distance = 8/' \
      "$REPO/third_party/SteelMC/package-content/config.toml" > "$SERVER/config/config.toml"
  fi
  (cd "$SERVER" && "$STEEL") & pids+=($!)
  echo "Local server starting on 127.0.0.1:25565"
fi

"$RELAY" --dir "$HERE/dist" --listen "127.0.0.1:$PORT" --allow-private-servers & pids+=($!)
until nc -z 127.0.0.1 "$PORT" 2>/dev/null; do sleep 0.3; done

URL="http://127.0.0.1:$PORT"
echo "Pomme is at $URL — Multiplayer -> Direct Connection -> 127.0.0.1"
open -a "Google Chrome" "$URL" 2>/dev/null || open "$URL" || true
wait
