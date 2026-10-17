#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PORT="${PORT:-4173}"
BIND="${BIND:-127.0.0.1}"

echo "TERRA is serving at http://localhost:${PORT}"
exec python3 -m http.server "$PORT" --directory "$ROOT/web" --bind "$BIND"
