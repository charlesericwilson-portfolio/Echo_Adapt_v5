#!/bin/bash
# run.sh - Run the agent (builds if needed)

set -e

echo "=== Running Echo Adapt v5 ==="

cd "$(dirname "$0")"

# Auto-build if binary missing
if [ ! -f "target/release/echo_rust_wrapper" ]; then
    echo "Binary not found. Building first..."
    ./build.sh
fi

echo "Launching agent..."
./target/release/Adapt_v5
