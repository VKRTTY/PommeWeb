#!/usr/bin/env bash
# One command to play: builds the page if it isn't built, starts the relay
# with the integrated local-world feature, and opens Chrome.
#   ./play.sh            play (builds only when dist/ is missing)
#   ./play.sh --build    rebuild first
# Then: Play -> Create New World or select an existing world.
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
(cd "$REPO" && cargo build -p pomme-relay --release --features local-worlds)

if nc -z 127.0.0.1 "$PORT" 2>/dev/null; then
  echo "Port $PORT is already in use; stop the other relay or set PORT." >&2
  exit 1
fi

pids=()
trap 'kill "${pids[@]}" 2>/dev/null || true' EXIT
"$RELAY" --dir "$HERE/dist" --listen "127.0.0.1:$PORT" --allow-private-servers \
  --local-worlds "${POMME_LOCAL_WORLDS:-$HERE/.server/worlds}" & pids+=($!)
until nc -z 127.0.0.1 "$PORT" 2>/dev/null; do kill -0 "${pids[0]}" 2>/dev/null || exit 1; sleep 0.3; done

URL="http://127.0.0.1:$PORT"
echo "Pomme is at $URL — Play opens your local worlds"
open -a "Google Chrome" "$URL" 2>/dev/null || open "$URL" || true
wait
