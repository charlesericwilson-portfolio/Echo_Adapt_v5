#!/bin/bash
# build.sh - Build the release executable

set -e

echo "=== Building Echo Adapt v5 ==="

cd "$(dirname "$0")"

cargo build --release

if [ -f "target/release/echo_rust_wrapper" ]; then
    echo "✅ Build successful!"
    echo "Binary: target/release/Adapt_v5"
else
    echo "❌ Build failed."
    exit 1
fi
