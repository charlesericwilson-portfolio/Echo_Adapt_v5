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

# Create model group and add user to it
sudo groupadd -f model
sudo usermod -aG model "$USER_NAME"

# Create workspace with proper permissions
sudo mkdir -p "$WORKSPACE"
sudo chown "$USER_NAME:model" "$WORKSPACE"
sudo chmod 755 "$WORKSPACE"   # owner full, group read/execute, others read/execute

# Allow safe apt commands (no dangerous ones)
sudo tee /etc/sudoers.d/model-user <<EOF
model-user ALL=(ALL) NOPASSWD: /usr/bin/apt update, /usr/bin/apt upgrade, /usr/bin/apt autoremove
EOF

echo ""
echo "Setup complete!"
echo "Restricted user: $USER_NAME"
echo "Workspace: $WORKSPACE (write only here)"
echo ""
echo "Run your model safely as:"
echo "  sudo -u $USER_NAME ./your-model-executable"
