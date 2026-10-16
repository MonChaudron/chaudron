#!/usr/bin/env sh
# cargo dans le conteneur toolbox, avec la base de dev. Ex. : scripts/cargo.sh test, scripts/cargo.sh clippy
set -e
# UPDATE_OPENAPI=1 scripts/cargo.sh test --test api : réécrit packages/api/openapi.json.
exec "$(dirname "$0")/dev.sh" run --rm -e UPDATE_OPENAPI="${UPDATE_OPENAPI:-}" toolbox cargo "$@"
