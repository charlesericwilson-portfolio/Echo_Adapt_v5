#!/usr/bin/env bash
set -euo pipefail

ARCH="${1:-}"

case "$ARCH" in
    rocm)
        echo "Building ADAPT Server for AMD ROCm..."
        cargo rustc \
            --release \
            --features rocm \
            --bin adapt_server \
            -- \
            -C link-arg=-no-pie
        ;;

    cuda)
        echo "Building ADAPT Server for NVIDIA CUDA..."
        cargo build \
            --release \
            --features cuda \
            --bin adapt_server
        ;;

    cpu)
        echo "Building ADAPT Server for CPU..."
        cargo build \
            --release \
            --bin adapt_server
        ;;

    *)
        echo "Usage:"
        echo "  ./build.sh rocm"
        echo "  ./build.sh cuda"
        echo "  ./build.sh cpu"
        echo
        echo "Examples:"
        echo "  ./build.sh rocm   # AMD GPU / ROCm"
        echo "  ./build.sh cuda   # NVIDIA GPU / CUDA"
        echo "  ./build.sh cpu    # CPU-only"
        exit 1
        ;;
esac

echo
echo "Build complete:"
echo "  target/release/adapt_server"
