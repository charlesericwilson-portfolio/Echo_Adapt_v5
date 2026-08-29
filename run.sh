#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
BINARY="$SCRIPT_DIR/target/release/Adapt_v5"

cd "$SCRIPT_DIR"

if [[ ! -x "$BINARY" ]]; then
    echo "Adapt_v5 has not been built. Building..."
    "$SCRIPT_DIR/build.sh"
fi

echo "=== Running Echo Adapt v5 ==="
exec "$BINARY"
