#!/bin/bash
# install-deps.sh - Install system + Rust dependencies

set -e

echo "=== Installing Echo v5 Dependencies ==="

# System packages
echo "Installing system dependencies..."
sudo apt update
sudo apt install -y tmux curl build-essential pkg-config libssl-dev

# Rust
if ! command -v cargo >/dev/null 2>&1; then
    echo "Installing Rust via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
    echo "Rust installed. Add 'source ~/.cargo/env' to your shell config if needed."
else
    echo "Rust already installed. Updating..."
    rustup update
fi

echo "✅ Dependencies installed!"
