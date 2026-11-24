#!/bin/sh
set -eu
domain=${POMME_PUBLIC_HOST:-${RAILWAY_PUBLIC_DOMAIN:-mcweb.up.railway.app}}
exec pomme-relay --dir /app/dist --listen "0.0.0.0:${PORT:-8080}" --public-host "$domain" --trust-proxy
