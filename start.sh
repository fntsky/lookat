#!/bin/sh
set -eu
project_root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$project_root"
if [ ! -x dist/lookat-server ]; then
    printf '%s\n' "Lookat server not found; run make build first." >&2
    exit 1
fi
export LOOKAT_DATA_PATH="${LOOKAT_DATA_PATH:-$project_root/data/lookat.sqlite3}"
export LOOKAT_PORT="${LOOKAT_PORT:-53214}"
export LOOKAT_FRONTEND_DIR="${LOOKAT_FRONTEND_DIR:-$project_root/frontend/dist}"
exec ./dist/lookat-server "$@"
