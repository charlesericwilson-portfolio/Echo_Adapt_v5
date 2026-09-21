#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"


echo "=== Building Echo Adapt v5 ==="
rm -f Cargo.lock
cargo update
cargo build --release --locked

ADAPT_BINARY="$SCRIPT_DIR/target/release/Adapt_v5"
echo "DEBUG SCRIPT_DIR=[$SCRIPT_DIR]"
echo "DEBUG ADAPT_BINARY=[$ADAPT_BINARY]"
if [[ ! -x "$ADAPT_BINARY" ]]; then
    echo "ERROR: Build completed but Adapt_v5 was not found."
    exit 1
fi

echo "=== Building Adapt Tool Server ==="
cd "$SCRIPT_DIR/tool_server"
rm -f Cargo.lock
cargo update
cargo build --release --locked

TOOL_SERVER_BINARY="$SCRIPT_DIR/tool_server/target/release/Adapt_tool_server"

if [[ ! -x "$TOOL_SERVER_BINARY" ]]; then
    echo "ERROR: Build completed but Adapt_tool_server was not found."
    exit 1
fi

cd "$SCRIPT_DIR"

echo "Build successful."
echo "Adapt binary: $ADAPT_BINARY"
echo "Tool server binary: $TOOL_SERVER_BINARY"
