#!/bin/bash
set -euo pipefail

echo "=== Setting up single restricted model user ==="

# === USER CONFIGURATION ===

USER_NAME="model-user"
GROUP_NAME="model"

WORKSPACE="/home/${USER_NAME}/model-workspace"
VENV_PATH="/home/${USER_NAME}/.venv"

# Commands the restricted model user may run through sudo without a password.
# Keep this list intentionally small. Add only commands you explicitly trust.
SUDO_COMMANDS=(
    "/usr/bin/apt update"
    "/usr/bin/apt install *"
)

# === END USER CONFIGURATION ===

# === PREFLIGHT CHECKS ===

if [[ "$(uname -s)" != "Linux" ]]; then
    echo "ERROR: Restricted model user setup requires Linux or WSL2."
    exit 1
fi

REQUIRED_COMMANDS=(
    sudo
    useradd
    groupadd
    usermod
    passwd
    python3
    visudo
    mktemp
    install
)

for command in "${REQUIRED_COMMANDS[@]}"; do
    if ! command -v "$command" >/dev/null 2>&1; then
        echo "ERROR: Required command '$command' was not found."
        exit 1
    fi
done

# Create the user if it doesn't exist
if ! id "$USER_NAME" &>/dev/null; then
    sudo useradd -m -s /bin/bash "$USER_NAME"
    echo "User $USER_NAME created"
else
    echo "User $USER_NAME already exists"
fi

# Lock password authentication for the model account.
# The account can still be launched by an administrator with sudo -u.
sudo passwd -l "$USER_NAME"

# Create model group and add user
sudo groupadd -f "$GROUP_NAME"
sudo usermod -aG "$GROUP_NAME" "$USER_NAME"

HOME_DIR="/home/${USER_NAME}"

# Keep the parent home directory owned by root so the model user
# cannot create arbitrary files directly under $HOME.
sudo chown root:root "$HOME_DIR"
sudo chmod 0755 "$HOME_DIR"

# Create the explicitly writable locations.
sudo mkdir -p "$WORKSPACE"
sudo mkdir -p "$VENV_PATH"

sudo chown -R "$USER_NAME:$GROUP_NAME" "$WORKSPACE"
sudo chown -R "$USER_NAME:$GROUP_NAME" "$VENV_PATH"

sudo chmod 0755 "$WORKSPACE"
sudo chmod 0755 "$VENV_PATH"

# Build the restricted sudo allowlist.
SUDOERS_FILE="/etc/sudoers.d/${USER_NAME}"
TEMP_SUDOERS="$(mktemp)"

if [[ ${#SUDO_COMMANDS[@]} -gt 0 ]]; then
    {
        printf '%s ALL=(root) NOPASSWD: ' "$USER_NAME"

        for i in "${!SUDO_COMMANDS[@]}"; do
            if [[ "$i" -gt 0 ]]; then
                printf ', '
            fi

            printf '%s' "${SUDO_COMMANDS[$i]}"
        done

        printf '\n'
    } > "$TEMP_SUDOERS"

    # Validate the generated sudoers policy before installing it.
    if ! sudo visudo -cf "$TEMP_SUDOERS"; then
        echo "ERROR: Generated sudoers configuration is invalid."
        rm -f "$TEMP_SUDOERS"
        exit 1
    fi

    sudo install -o root -g root -m 0440 \
        "$TEMP_SUDOERS" \
        "$SUDOERS_FILE"

    rm -f "$TEMP_SUDOERS"

    echo "Installed sudo allowlist: $SUDOERS_FILE"
else
    echo "No passwordless sudo commands configured."

    # Remove an old policy from a previous run if one exists.
    sudo rm -f "$SUDOERS_FILE"
fi

# Create the model user's persistent Python virtual environment.
if [[ ! -x "$VENV_PATH/bin/python" ]]; then
    echo "Creating Python virtual environment at $VENV_PATH..."

    sudo -u "$USER_NAME" python3 -m venv "$VENV_PATH"
else
    echo "Python virtual environment already exists."
fi

# Verify that the venv provides the plain `python` and `pip` commands
# expected by model workflows.
if [[ ! -x "$VENV_PATH/bin/python" ]]; then
    echo "ERROR: Python executable was not created in the virtual environment."
    exit 1
fi

if [[ ! -x "$VENV_PATH/bin/pip" ]]; then
    echo "ERROR: pip was not created in the virtual environment."
    exit 1
fi

echo "Python environment ready:"
echo "  $VENV_PATH/bin/python"
echo "  $VENV_PATH/bin/pip"

# Configure interactive shells to use the model user's Python venv.
# This file is root-owned so the restricted model user cannot modify
# its own shell initialization policy.
sudo tee "$HOME_DIR/.bashrc" >/dev/null <<EOF
# Managed by Echo Adapt restricted model user setup.
export VIRTUAL_ENV="$VENV_PATH"
export PATH="$VENV_PATH/bin:\$PATH"
EOF

sudo chown root:root "$HOME_DIR/.bashrc"
sudo chmod 0644 "$HOME_DIR/.bashrc"

echo ""
echo "Setup complete!"
echo "User: $USER_NAME (password login disabled)"
echo "Workspace: $WORKSPACE"
echo "Python venv: $VENV_PATH"
echo "Writable locations: workspace and Python venv"
echo ""
echo "Run Adapt as restricted model user:"
echo "  sudo -H -u $USER_NAME ./target/release/Adapt_v5"
