#!/bin/sh
set -eu
project_root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$project_root"
if [ ! -x dist/lookat-server ] || [ ! -f frontend/dist/index.html ]; then
    printf '%s\n' "Building Lookat Chamber for first launch..." >&2
    for tool in make cargo npm; do
        if ! command -v "$tool" >/dev/null 2>&1; then
            printf '%s\n' "Missing build tool: $tool. Install Rust, Node.js/npm and make, then restart the Chamber." >&2
            exit 1
        fi
    done
    make build >&2
fi
export LOOKAT_DATA_PATH="${LOOKAT_DATA_PATH:-$project_root/data/lookat.sqlite3}"
export LOOKAT_PORT="${LOOKAT_PORT:-53214}"
export LOOKAT_FRONTEND_DIR="${LOOKAT_FRONTEND_DIR:-$project_root/frontend/dist}"
exec ./dist/lookat-server "$@"
