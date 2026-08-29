#!/usr/bin/env bash
# install_deps.sh - Install system and Rust dependencies

set -euo pipefail

echo "=== Installing Echo Adapt v5 Dependencies ==="

echo "Detecting package manager..."

if command -v apt-get >/dev/null 2>&1; then
    echo "Detected apt (Debian/Ubuntu family)"
    sudo apt-get update
    sudo apt-get install -y tmux curl build-essential pkg-config

elif command -v dnf >/dev/null 2>&1; then
    echo "Detected dnf (Fedora/RHEL family)"
    sudo dnf install -y tmux curl gcc gcc-c++ make pkgconf-pkg-config

elif command -v pacman >/dev/null 2>&1; then
    echo "Detected pacman (Arch/Manjaro family)"
    sudo pacman -Syu --needed --noconfirm tmux curl base-devel pkgconf

elif command -v zypper >/dev/null 2>&1; then
    echo "Detected zypper (openSUSE family)"
    sudo zypper install -y tmux curl gcc gcc-c++ make pkg-config

else
    echo "ERROR: Unsupported package manager."
    echo "Please install these dependencies manually:"
    echo "  tmux"
    echo "  curl"
    echo "  C/C++ build tools"
    echo "  pkg-config"
    exit 1
fi

# Install Rust only if Cargo is missing.
if ! command -v cargo >/dev/null 2>&1; then
    echo "Cargo not found. Installing Rust via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y

    # Make Cargo available to this script immediately.
    # shellcheck disable=SC1090
    source "$HOME/.cargo/env"
else
    echo "Rust/Cargo already installed."
fi

echo
echo "Verifying dependencies..."

for command in tmux curl cargo; do
    if ! command -v "$command" >/dev/null 2>&1; then
        echo "ERROR: Required command '$command' was not found."
        exit 1
    fi
done

echo "Dependencies installed successfully."✅
