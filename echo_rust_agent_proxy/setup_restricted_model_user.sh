#!/bin/bash
set -e

echo "=== Setting up single restricted model user ==="

USER_NAME="model-user"
WORKSPACE="/home/${USER_NAME}/model-workspace"

# Create the user if it doesn't exist
if ! id "$USER_NAME" &>/dev/null; then
    sudo useradd -m -s /bin/bash "$USER_NAME"
    echo "User $USER_NAME created"
else
    echo "User $USER_NAME already exists"
fi

# Make the account passwordless
sudo passwd -d "$USER_NAME"

# Create model group and add user
sudo groupadd -f model
sudo usermod -aG model "$USER_NAME"

# Create workspace with proper permissions
sudo mkdir -p "$WORKSPACE"
sudo chown "$USER_NAME:model" "$WORKSPACE"
sudo chmod 755 "$WORKSPACE"

# Allow safe apt commands
sudo tee /etc/sudoers.d/model-user <<EOF
model-user ALL=(ALL) NOPASSWD: /usr/bin/apt update, /usr/bin/apt upgrade, /usr/bin/apt autoremove
EOF

echo ""
echo "Setup complete!"
echo "User: $USER_NAME (passwordless)"
echo "Workspace: $WORKSPACE (write only here)"
echo ""
echo "Switch to user:"
echo "  su - $USER_NAME"
echo "Run model:"
echo "  sudo -u $USER_NAME ./your-model-executable"
