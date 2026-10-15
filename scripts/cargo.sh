#!/usr/bin/env sh
# cargo dans le conteneur toolbox, avec la base de dev. Ex. : scripts/cargo.sh test, scripts/cargo.sh clippy
set -e
exec "$(dirname "$0")/dev.sh" run --rm toolbox cargo "$@"
