#!/bin/sh
set -eu
domain=${POMME_PUBLIC_HOST:-${RAILWAY_PUBLIC_DOMAIN:-mcweb.up.railway.app}}
# Singleplayer worlds live on a Railway volume when one is mounted (and
# writable), else in the container, where a redeploy wipes them.
worlds="${RAILWAY_VOLUME_MOUNT_PATH:-/tmp}/pomme-worlds"
mkdir -p "$worlds" 2>/dev/null && [ -w "$worlds" ] || { worlds=/tmp/pomme-worlds; mkdir -p "$worlds"; }
exec pomme-relay --dir /app/dist --listen "0.0.0.0:${PORT:-8080}" --public-host "$domain" --trust-proxy \
  --local-worlds "$worlds" --hosted-worlds "${POMME_HOSTED_WORLDS:-2}"
