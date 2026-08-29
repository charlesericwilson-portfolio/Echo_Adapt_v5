#!/usr/bin/env bash
set -euo pipefail

# === USER CONFIGURATION ===

MODEL_USER="model-user"

# === END USER CONFIGURATION ===

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
BINARY="$SCRIPT_DIR/target/release/Adapt_v5"

cd "$SCRIPT_DIR"

if [[ ! -x "$BINARY" ]]; then
    echo "Adapt_v5 has not been built. Building..."
    "$SCRIPT_DIR/build.sh"
fi

case "${1:-}" in
    --restricted)
        if ! id "$MODEL_USER" >/dev/null 2>&1; then
            echo "ERROR: Restricted model user '$MODEL_USER' does not exist."
            echo "Run setup_restricted_model_user.sh first."
            exit 1
        fi

        echo "=== Running Echo Adapt v5 as $MODEL_USER ==="

        exec sudo -H -u "$MODEL_USER" \
            "$BINARY"
        ;;

    "")
        echo "=== Running Echo Adapt v5 as $(whoami) ==="
        exec "$BINARY"
        ;;

    *)
        echo "Usage:"
        echo "  ./run.sh"
        echo "  ./run.sh --restricted"
        exit 1
        ;;
esac
