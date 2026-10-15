#!/usr/bin/env sh
# docker compose avec la surcouche de dev. Ex. : scripts/dev.sh up --build, scripts/dev.sh down
set -e
deploy="$(cd "$(dirname "$0")/../deploy" && pwd)"
[ -f "$deploy/.env" ] || cp "$deploy/.env.example" "$deploy/.env"
exec docker compose -f "$deploy/docker-compose.yml" -f "$deploy/docker-compose.dev.yml" "$@"
