#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "=== Building Echo Adapt v5 ==="

cargo build --release --locked

BINARY="$SCRIPT_DIR/target/release/Adapt_v5"

if [[ ! -x "$BINARY" ]]; then
    echo "ERROR: Build completed but Adapt_v5 was not found."
    exit 1
fi

echo "Build successful."
echo "Binary: $BINARY"
