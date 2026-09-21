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

TOOL_SERVER_DIR="$SCRIPT_DIR/tool_server"
TOOL_SERVER_BINARY="$TOOL_SERVER_DIR/target/release/Adapt_tool_server"
TOOL_SERVER_HEALTH="http://127.0.0.1:9000/health"

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
        # Start remote tool server as the signed-in user.
        # ----------------------------------------------------

        echo "=== Starting Adapt Tool Server ==="

        (
            cd "$TOOL_SERVER_DIR"
            exec "$TOOL_SERVER_BINARY"
        ) &

        TOOL_SERVER_PID=$!

        echo "Waiting for Adapt Tool Server..."

        for _ in {1..50}; do
            if curl -fsS "$TOOL_SERVER_HEALTH" >/dev/null 2>&1; then
                echo "Adapt Tool Server ready."
                break
            fi

            if ! kill -0 "$TOOL_SERVER_PID" 2>/dev/null; then
                echo "ERROR: Adapt Tool Server exited before becoming ready."
                exit 1
            fi

            sleep 0.2
        done

        if ! curl -fsS "$TOOL_SERVER_HEALTH" >/dev/null 2>&1; then
            echo "ERROR: Adapt Tool Server did not become ready."
            kill "$TOOL_SERVER_PID" 2>/dev/null || true
            exit 1
        fi

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

        DONE_FILE="$(mktemp)"
        rm -f "$DONE_FILE"

        LAUNCH_COMMAND="cd '$MODEL_HOME'; sudo -H -u '$MODEL_USER' '$MODEL_BINARY'; ADAPT_EXIT=\$?; printf '%s\n' \"\$ADAPT_EXIT\" > '$DONE_FILE'"

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

        echo "Waiting for restricted Adapt to exit..."

        while [[ ! -f "$DONE_FILE" ]]; do
            sleep 0.5
        done

        ADAPT_EXIT="$(cat "$DONE_FILE")"
        rm -f "$DONE_FILE"

        if kill -0 "$TOOL_SERVER_PID" 2>/dev/null; then
            echo "=== Stopping Adapt Tool Server ==="
            kill "$TOOL_SERVER_PID" 2>/dev/null || true
            wait "$TOOL_SERVER_PID" 2>/dev/null || true
        fi

        exit "$ADAPT_EXIT"
        ;;

    # --------------------------------------------------------
    # LOCKDOWN MODEL USER
    # --------------------------------------------------------

    --lockdown)

        if ! id "$MODEL_USER" >/dev/null 2>&1; then
            echo "ERROR: Restricted model user '$MODEL_USER' does not exist."
            echo "Run setup_restricted_model_user.sh first."
            exit 1
        fi

        if ! command -v bwrap >/dev/null 2>&1; then
            echo "ERROR: Bubblewrap was not found."
            echo "Run setup_lockdown.sh first."
            exit 1
        fi

        # ----------------------------------------------------
        # Verify Bubblewrap works before changing anything.
        # ----------------------------------------------------

        if ! bwrap \
            --ro-bind / / \
            --proc /proc \
            --dev /dev \
            --tmpfs /tmp \
            --unshare-pid \
            --new-session \
            /bin/true \
            >/dev/null 2>&1
        then
            echo "ERROR: Bubblewrap lockdown prerequisites are not ready."
            echo "Run setup_lockdown.sh first."
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
            exit 1
        fi

        echo "Detected terminal: $TERMINAL"
        echo "=== Preparing lockdown Adapt environment ==="

        # ----------------------------------------------------
        # Stage the current Adapt executable.
        # ----------------------------------------------------

        sudo rm -f "$MODEL_BINARY"
        sudo cp "$BINARY" "$MODEL_BINARY"
        sudo chown root:root "$MODEL_BINARY"
        sudo chmod 0755 "$MODEL_BINARY"

        # ----------------------------------------------------
        # Stage configuration and prompts.
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

        # ----------------------------------------------------
        # model-user owns its home.
        # ----------------------------------------------------

        sudo chown "$MODEL_USER:$MODEL_USER" "$MODEL_HOME"
        sudo chmod 0755 "$MODEL_HOME"

        # ----------------------------------------------------
        # Persistent SQLite database.
        # ----------------------------------------------------

        if [[ ! -f "$MODEL_DB" ]]; then
            if [[ -f "$SCRIPT_DIR/echo_tools.db" ]]; then
                echo "Initializing lockdown tool database..."
                sudo cp "$SCRIPT_DIR/echo_tools.db" "$MODEL_DB"
            else
                echo "Creating lockdown tool database..."
                sudo touch "$MODEL_DB"
            fi
        fi

        sudo chown "$MODEL_USER:$MODEL_USER" "$MODEL_DB"
        sudo chmod 0644 "$MODEL_DB"

        # ----------------------------------------------------
        # Persistent JSONL conversation log.
        # ----------------------------------------------------

        if [[ ! -f "$MODEL_CHAT_LOG" ]]; then
            echo "Creating lockdown conversation log..."
            sudo touch "$MODEL_CHAT_LOG"
        fi

        sudo chown "$MODEL_USER:$MODEL_USER" "$MODEL_CHAT_LOG"
        sudo chmod 0644 "$MODEL_CHAT_LOG"

        # ----------------------------------------------------
        # Persistent workspace.
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
        # Persistent tmux socket location.
        #
        # Lockdown gets a private /tmp. tmux normally stores its
        # server socket under /tmp, which would disappear when
        # the sandbox closes.
        #
        # TMUX_TMPDIR keeps it under model-user's persistent home.
        # ----------------------------------------------------

        MODEL_TMUX_DIR="$MODEL_HOME/.tmux"

        sudo mkdir -p "$MODEL_TMUX_DIR"
        sudo chown "$MODEL_USER:$MODEL_USER" "$MODEL_TMUX_DIR"
        sudo chmod 0700 "$MODEL_TMUX_DIR"

        # ----------------------------------------------------
        # Host home to hide.
        #
        # Capture this BEFORE switching to model-user.
        # ----------------------------------------------------

        HOST_HOME="$HOME"

                # ----------------------------------------------------
        # Start remote tool server as the signed-in user.
        # ----------------------------------------------------

        echo "=== Starting Adapt Tool Server ==="

        (
            cd "$TOOL_SERVER_DIR"
            exec "$TOOL_SERVER_BINARY"
        ) &

        TOOL_SERVER_PID=$!

        echo "Waiting for Adapt Tool Server..."

        for _ in {1..50}; do
            if curl -fsS "$TOOL_SERVER_HEALTH" >/dev/null 2>&1; then
                echo "Adapt Tool Server ready."
                break
            fi

            if ! kill -0 "$TOOL_SERVER_PID" 2>/dev/null; then
                echo "ERROR: Adapt Tool Server exited before becoming ready."
                exit 1
            fi

            sleep 0.2
        done

        if ! curl -fsS "$TOOL_SERVER_HEALTH" >/dev/null 2>&1; then
            echo "ERROR: Adapt Tool Server did not become ready."
            kill "$TOOL_SERVER_PID" 2>/dev/null || true
            exit 1
        fi

        echo "=== Launching Echo Adapt v5 in LOCKDOWN mode ==="
        echo "Terminal: $TERMINAL"
        echo "Model user: $MODEL_USER"
        echo "Networking: enabled"
        echo "Host home hidden: $HOST_HOME"

        # ----------------------------------------------------
        # Bubblewrap layout
        #
        # Start with the host filesystem read-only so normal
        # binaries/libraries still function.
        #
        # Then:
        #   - hide the signed-in user's home
        #   - hide common removable/mounted-data locations
        #   - expose model-user's home read/write
        #   - re-bind critical runtime files read-only
        #   - provide private /tmp
        #   - provide fresh /proc and /dev
        #
        # Networking is intentionally NOT unshared.
        # ----------------------------------------------------
        DONE_FILE="$(mktemp)"
        rm -f "$DONE_FILE"
        LOCKDOWN_COMMAND="
            sudo -H -u '$MODEL_USER' \
            bwrap \
                --die-with-parent \
                --new-session \
                --unshare-pid \
                --unshare-ipc \
                --unshare-uts \
                --ro-bind / / \
                --tmpfs '$HOST_HOME' \
                --tmpfs /root \
                --tmpfs /mnt \
                --tmpfs /media \
                --bind '$MODEL_HOME' '$MODEL_HOME' \
                --ro-bind '$MODEL_BINARY' '$MODEL_BINARY' \
                --ro-bind '$MODEL_HOME/config.toml' '$MODEL_HOME/config.toml' \
                --ro-bind '$MODEL_HOME/main_system.txt' '$MODEL_HOME/main_system.txt' \
                --ro-bind '$MODEL_HOME/summarizer.txt' '$MODEL_HOME/summarizer.txt' \
                --bind "$MODEL_DB" "$MODEL_DB" \
                --bind "$MODEL_CHAT_LOG" "$MODEL_CHAT_LOG" \
                --proc /proc \
                --dev /dev \
                --tmpfs /tmp \
                --setenv HOME '$MODEL_HOME' \
                --setenv USER '$MODEL_USER' \
                --setenv LOGNAME '$MODEL_USER' \
                --setenv VIRTUAL_ENV '$MODEL_HOME/.venv' \
                --setenv TMUX_TMPDIR '$MODEL_TMUX_DIR' \
                --setenv PATH '$MODEL_HOME/.venv/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin' \
                --chdir '$MODEL_HOME' \
                '$MODEL_BINARY'

                ADAPT_EXIT=\$?

                echo
                echo \"========================================\"
                echo \"Adapt exited with code: \$ADAPT_EXIT\"
                echo \"========================================\"

                printf '%s\n' \"\$ADAPT_EXIT\" > '$DONE_FILE'

                echo
                read -rp \"Press Enter to close this window...\"
        "

        case "$TERMINAL" in

            konsole)
                konsole -e bash -lc "$LOCKDOWN_COMMAND"
                ;;

            gnome-terminal)
                gnome-terminal -- bash -lc "$LOCKDOWN_COMMAND"
                ;;

            kitty)
                kitty bash -lc "$LOCKDOWN_COMMAND"
                ;;

            alacritty)
                alacritty -e bash -lc "$LOCKDOWN_COMMAND"
                ;;

            xfce4-terminal)
                xfce4-terminal --command="bash -lc \"$LOCKDOWN_COMMAND\""
                ;;

            xterm)
                xterm -e bash -lc "$LOCKDOWN_COMMAND"
                ;;

        esac

        echo "Waiting for lockdown Adapt to exit..."

        while [[ ! -f "$DONE_FILE" ]]; do
            sleep 0.5
        done

        ADAPT_EXIT="$(cat "$DONE_FILE")"
        rm -f "$DONE_FILE"

        if kill -0 "$TOOL_SERVER_PID" 2>/dev/null; then
            echo "=== Stopping Adapt Tool Server ==="
            kill "$TOOL_SERVER_PID" 2>/dev/null || true
            wait "$TOOL_SERVER_PID" 2>/dev/null || true
        fi

        exit "$ADAPT_EXIT"
        ;;

    # --------------------------------------------------------
    # NORMAL CURRENT-USER MODE
    # --------------------------------------------------------

    "")

        echo "=== Starting Adapt Tool Server ==="

        (
            cd "$TOOL_SERVER_DIR"
            exec "$TOOL_SERVER_BINARY"
        ) &

        TOOL_SERVER_PID=$!

        cleanup_tool_server() {
            if kill -0 "$TOOL_SERVER_PID" 2>/dev/null; then
                echo "=== Stopping Adapt Tool Server ==="
                kill "$TOOL_SERVER_PID" 2>/dev/null || true
                wait "$TOOL_SERVER_PID" 2>/dev/null || true
            fi
        }

        trap cleanup_tool_server EXIT INT TERM

        echo "Waiting for Adapt Tool Server..."

        for _ in {1..50}; do
            if curl -fsS "$TOOL_SERVER_HEALTH" >/dev/null 2>&1; then
                echo "Adapt Tool Server ready."
                break
            fi

            if ! kill -0 "$TOOL_SERVER_PID" 2>/dev/null; then
                echo "ERROR: Adapt Tool Server exited before becoming ready."
                exit 1
            fi

            sleep 0.2
        done

        if ! curl -fsS "$TOOL_SERVER_HEALTH" >/dev/null 2>&1; then
            echo "ERROR: Adapt Tool Server did not become ready."
            exit 1
        fi

        echo "=== Running Echo Adapt v5 as $(whoami) ==="

        set +e
        "$BINARY"
        ADAPT_EXIT=$?
        set -e

        exit "$ADAPT_EXIT"
        ;;

    # --------------------------------------------------------
    # INVALID OPTION
    # --------------------------------------------------------

    *)

        echo "Usage:"
        echo "  ./run.sh"
        echo "  ./run.sh --restricted"
        echo "  ./run.sh --lockdown"
        exit 1
        ;;

esac
