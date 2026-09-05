#!/usr/bin/env bash
set -euo pipefail

# ============================================================
# USER CONFIGURATION
# ============================================================

MODEL_USER="model-user"

# ============================================================
# PATHS
# ============================================================

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

BINARY="$SCRIPT_DIR/target/release/Adapt_v5"

MODEL_HOME="/home/$MODEL_USER"
MODEL_BINARY="$MODEL_HOME/Adapt_v5"
MODEL_DB="$MODEL_HOME/echo_tools.db"
MODEL_CHAT_LOG="$MODEL_HOME/echo_chat.jsonl"
MODEL_WORKSPACE="$MODEL_HOME/workspace"

cd "$SCRIPT_DIR"

# ============================================================
# BUILD
# ============================================================

if [[ ! -x "$BINARY" ]]; then
    echo "Adapt_v5 has not been built. Building..."
    "$SCRIPT_DIR/build.sh"
fi

# ============================================================
# RUN MODE
# ============================================================

case "${1:-}" in

    # --------------------------------------------------------
    # RESTRICTED MODEL USER
    # --------------------------------------------------------

    --restricted)

        if ! id "$MODEL_USER" >/dev/null 2>&1; then
            echo "ERROR: Restricted model user '$MODEL_USER' does not exist."
            echo "Run setup_restricted_model_user.sh first."
            exit 1
        fi

            # ----------------------------------------------------
        # Detect an available terminal emulator.
        # ----------------------------------------------------

        TERMINAL=""

        if command -v konsole >/dev/null 2>&1; then
            TERMINAL="konsole"

        elif command -v gnome-terminal >/dev/null 2>&1; then
            TERMINAL="gnome-terminal"

        elif command -v kitty >/dev/null 2>&1; then
            TERMINAL="kitty"

        elif command -v alacritty >/dev/null 2>&1; then
            TERMINAL="alacritty"

        elif command -v xfce4-terminal >/dev/null 2>&1; then
            TERMINAL="xfce4-terminal"

        elif command -v xterm >/dev/null 2>&1; then
            TERMINAL="xterm"

        else
            echo "ERROR: No supported terminal emulator was found."
            echo
            echo "Supported terminals:"
            echo "  Konsole"
            echo "  GNOME Terminal"
            echo "  Kitty"
            echo "  Alacritty"
            echo "  XFCE Terminal"
            echo "  xterm"
            exit 1
        fi

        echo "Detected terminal: $TERMINAL"

        echo "=== Preparing restricted Adapt environment ==="

        # ----------------------------------------------------
        # Copy the current Adapt executable.
        #
        # The model user does not need access to the developer's
        # home directory or Git repository.
        # ----------------------------------------------------
        sudo rm -f "$MODEL_BINARY"
        sudo cp "$BINARY" "$MODEL_BINARY"
        sudo chown root:root "$MODEL_BINARY"
        sudo chmod 0755 "$MODEL_BINARY"

        # ----------------------------------------------------
        # Copy runtime configuration and prompts.
        #
        # These remain root-owned so the restricted model user
        # can read them but cannot silently modify its own
        # runtime configuration or system prompts.
        # ----------------------------------------------------

        sudo cp "$SCRIPT_DIR/config.toml" \
            "$MODEL_HOME/config.toml"

        sudo cp "$SCRIPT_DIR/main_system.txt" \
            "$MODEL_HOME/main_system.txt"

        sudo cp "$SCRIPT_DIR/summarizer.txt" \
            "$MODEL_HOME/summarizer.txt"

        sudo chown root:root \
            "$MODEL_HOME/config.toml" \
            "$MODEL_HOME/main_system.txt" \
            "$MODEL_HOME/summarizer.txt"

        sudo chmod 0644 \
            "$MODEL_HOME/config.toml" \
            "$MODEL_HOME/main_system.txt" \
            "$MODEL_HOME/summarizer.txt"
        sudo chown model-user:model-user /home/model-user
        sudo chmod 0755 /home/model-user
        # ----------------------------------------------------
        # SQLite database.
        #
        # Preserve the restricted user's database between
        # launches. Initialize it only if it does not exist.
        # ----------------------------------------------------

        if [[ ! -f "$MODEL_DB" ]]; then
            if [[ -f "$SCRIPT_DIR/echo_tools.db" ]]; then
                echo "Initializing restricted tool database..."
                sudo cp "$SCRIPT_DIR/echo_tools.db" "$MODEL_DB"
            else
                echo "Creating restricted tool database..."
                sudo touch "$MODEL_DB"
            fi
        fi

        sudo chown "$MODEL_USER:$MODEL_USER" "$MODEL_DB"
        sudo chmod 0644 "$MODEL_DB"

        # ----------------------------------------------------
        # JSONL conversation log.
        #
        # Restricted mode keeps its own persistent transcript.
        # Create it once and preserve it between launches.
        # ----------------------------------------------------

        if [[ ! -f "$MODEL_CHAT_LOG" ]]; then
            echo "Creating restricted conversation log..."
            sudo touch "$MODEL_CHAT_LOG"
        fi

        sudo chown "$MODEL_USER:$MODEL_USER" "$MODEL_CHAT_LOG"
        sudo chmod 0644 "$MODEL_CHAT_LOG"

        # ----------------------------------------------------
        # Persistent workspace.
        #
        # Keep the same relative paths used by Adapt workflows:
        #
        # workspace/temp
        # workspace/human_review
        # workspace/scripts
        # ----------------------------------------------------

        sudo mkdir -p \
            "$MODEL_WORKSPACE/temp" \
            "$MODEL_WORKSPACE/human_review" \
            "$MODEL_WORKSPACE/scripts"

        sudo chown -R \
            "$MODEL_USER:$MODEL_USER" \
            "$MODEL_WORKSPACE"

        sudo chmod -R 0755 "$MODEL_WORKSPACE"

       # ----------------------------------------------------
        # Launch Adapt in a new terminal window.
        #
        # sudo -u changes process identity but does not create
        # a new foreground terminal on its own.
        #
        # The detected terminal emulator creates the new TTY,
        # then Adapt is launched inside it as model-user.
        # ----------------------------------------------------

       echo "=== Launching Echo Adapt v5 as $MODEL_USER ==="
        echo "Terminal: $TERMINAL"

        LAUNCH_COMMAND="cd '$MODEL_HOME' && exec sudo -H -u '$MODEL_USER' '$MODEL_BINARY'"

        case "$TERMINAL" in

            konsole)
                konsole -e bash -lc "$LAUNCH_COMMAND"
                ;;

            gnome-terminal)
                gnome-terminal -- bash -lc "$LAUNCH_COMMAND"
                ;;

            kitty)
                kitty bash -lc "$LAUNCH_COMMAND"
                ;;

            alacritty)
                alacritty -e bash -lc "$LAUNCH_COMMAND"
                ;;

            xfce4-terminal)
                xfce4-terminal --command="bash -lc \"$LAUNCH_COMMAND\""
                ;;

            xterm)
                xterm -e bash -lc "$LAUNCH_COMMAND"
                ;;

        esac

        exit 0
        ;;

    # --------------------------------------------------------
    # NORMAL CURRENT-USER MODE
    # --------------------------------------------------------

    "")

        echo "=== Running Echo Adapt v5 as $(whoami) ==="
        exec "$BINARY"
        ;;

    # --------------------------------------------------------
    # INVALID OPTION
    # --------------------------------------------------------

    *)

        echo "Usage:"
        echo "  ./run.sh"
        echo "  ./run.sh --restricted"
        exit 1
        ;;

esac
