#!/usr/bin/env bash
set -euo pipefail

# ============================================================
# Echo Adapt v5 - Lockdown Setup
#
# Prepares Bubblewrap for Adapt's --lockdown mode.
#
# Ubuntu may allow unprivileged user namespaces at the kernel
# level while restricting them through AppArmor. This script
# installs a narrowly scoped AppArmor profile for Bubblewrap
# rather than disabling that protection globally.
# ============================================================

BWRAP_BIN="$(command -v bwrap || true)"
APPARMOR_PROFILE="/etc/apparmor.d/usr.bin.bwrap"

echo "=== Echo Adapt v5 - Lockdown Setup ==="
echo

# ============================================================
# PLATFORM CHECK
# ============================================================

if [[ "$(uname -s)" != "Linux" ]]; then
    echo "ERROR: Lockdown setup currently supports Linux only."
    exit 1
fi

# Read distro information when available.
if [[ -r /etc/os-release ]]; then
    # shellcheck disable=SC1091
    source /etc/os-release
else
    ID="unknown"
    ID_LIKE=""
fi

echo "Detected Linux distribution: ${PRETTY_NAME:-unknown}"
echo

# ============================================================
# INSTALL BUBBLEWRAP
# ============================================================

if [[ -z "$BWRAP_BIN" ]]; then
    echo "Bubblewrap is not installed."

    if command -v apt-get >/dev/null 2>&1; then
        echo "Installing Bubblewrap..."
        sudo apt-get update
        sudo apt-get install -y bubblewrap

    elif command -v dnf >/dev/null 2>&1; then
        echo "Installing Bubblewrap..."
        sudo dnf install -y bubblewrap

    elif command -v pacman >/dev/null 2>&1; then
        echo "Installing Bubblewrap..."
        sudo pacman -S --needed bubblewrap

    elif command -v zypper >/dev/null 2>&1; then
        echo "Installing Bubblewrap..."
        sudo zypper install -y bubblewrap

    else
        echo "ERROR: Could not determine how to install Bubblewrap."
        echo "Install 'bubblewrap' manually and run this script again."
        exit 1
    fi

    BWRAP_BIN="$(command -v bwrap || true)"
fi

if [[ -z "$BWRAP_BIN" ]]; then
    echo "ERROR: Bubblewrap could not be found after installation."
    exit 1
fi

echo "Bubblewrap: $BWRAP_BIN"
bwrap --version
echo

# ============================================================
# BASIC USER-NAMESPACE INFORMATION
# ============================================================

echo "=== User Namespace Status ==="

if [[ -r /proc/sys/kernel/unprivileged_userns_clone ]]; then
    USERNS_CLONE="$(cat /proc/sys/kernel/unprivileged_userns_clone)"
    echo "kernel.unprivileged_userns_clone=$USERNS_CLONE"

    if [[ "$USERNS_CLONE" != "1" ]]; then
        echo
        echo "ERROR: Unprivileged user namespaces are disabled."
        echo "Adapt will not modify this global kernel setting automatically."
        exit 1
    fi
else
    echo "kernel.unprivileged_userns_clone: not exposed by this kernel"
fi

APPARMOR_USERNS_RESTRICT=""

if [[ -r /proc/sys/kernel/apparmor_restrict_unprivileged_userns ]]; then
    APPARMOR_USERNS_RESTRICT="$(
        cat /proc/sys/kernel/apparmor_restrict_unprivileged_userns
    )"

    echo "kernel.apparmor_restrict_unprivileged_userns=$APPARMOR_USERNS_RESTRICT"
fi

if [[ -r /proc/sys/kernel/apparmor_restrict_unprivileged_unconfined ]]; then
    echo "kernel.apparmor_restrict_unprivileged_unconfined=$(
        cat /proc/sys/kernel/apparmor_restrict_unprivileged_unconfined
    )"
fi

echo

# ============================================================
# INITIAL BWRAP SELF-TEST
#
# If Bubblewrap already works, don't modify AppArmor.
# ============================================================

echo "=== Testing Bubblewrap ==="

if bwrap \
    --ro-bind / / \
    --proc /proc \
    --dev /dev \
    --tmpfs /tmp \
    --unshare-pid \
    --new-session \
    /bin/true \
    >/dev/null 2>&1
then
    echo "Bubblewrap already works."
    echo
    echo "=== Lockdown prerequisites ready ==="
    exit 0
fi

echo "Initial Bubblewrap self-test failed."
echo

# ============================================================
# APPARMOR HANDLING
#
# Ubuntu may restrict unprivileged user namespaces through
# AppArmor even though unprivileged_userns_clone is enabled.
#
# We do NOT globally disable:
#
# kernel.apparmor_restrict_unprivileged_userns
#
# Instead, install a profile specifically for bwrap.
# ============================================================

if [[ "$APPARMOR_USERNS_RESTRICT" == "1" ]]; then

    echo "AppArmor is restricting unprivileged user namespaces."
    echo "Configuring a Bubblewrap-specific AppArmor allowance..."
    echo

    if ! command -v apparmor_parser >/dev/null 2>&1; then

        if command -v apt-get >/dev/null 2>&1; then
            echo "Installing AppArmor utilities..."
            sudo apt-get update
            sudo apt-get install -y apparmor apparmor-utils
        else
            echo "ERROR: apparmor_parser is required but was not found."
            echo "Install the AppArmor utilities for this distribution."
            exit 1
        fi
    fi

    # --------------------------------------------------------
    # Create profile only when it does not already exist.
    # Never silently overwrite an administrator's profile.
    # --------------------------------------------------------

    if [[ ! -f "$APPARMOR_PROFILE" ]]; then

        echo "Creating AppArmor profile:"
        echo "  $APPARMOR_PROFILE"

        TMP_PROFILE="$(mktemp)"

        cat > "$TMP_PROFILE" <<'EOF'
abi <abi/4.0>,
#include <tunables/global>

profile bwrap /usr/bin/bwrap flags=(unconfined) {
    userns,
}
EOF

        # Validate BEFORE installing.
        echo "Validating AppArmor profile..."

        if ! sudo apparmor_parser -Q "$TMP_PROFILE"; then
            echo "ERROR: Generated AppArmor profile failed validation."
            rm -f "$TMP_PROFILE"
            exit 1
        fi

        sudo install \
            -o root \
            -g root \
            -m 0644 \
            "$TMP_PROFILE" \
            "$APPARMOR_PROFILE"

        rm -f "$TMP_PROFILE"

    else
        echo "Existing Bubblewrap AppArmor profile found."
        echo "Leaving existing profile unchanged:"
        echo "  $APPARMOR_PROFILE"
    fi

    # --------------------------------------------------------
    # Validate installed profile.
    # --------------------------------------------------------

    echo "Validating installed AppArmor profile..."

    sudo apparmor_parser -Q "$APPARMOR_PROFILE"

    # --------------------------------------------------------
    # Load/reload profile.
    # --------------------------------------------------------

    echo "Loading Bubblewrap AppArmor profile..."

    sudo apparmor_parser -r "$APPARMOR_PROFILE"

    echo
fi

# ============================================================
# VERIFY GLOBAL PROTECTION WAS NOT DISABLED
# ============================================================

if [[ -r /proc/sys/kernel/apparmor_restrict_unprivileged_userns ]]; then

    CURRENT_RESTRICTION="$(
        cat /proc/sys/kernel/apparmor_restrict_unprivileged_userns
    )"

    echo "AppArmor global userns restriction: $CURRENT_RESTRICTION"

    if [[ "$APPARMOR_USERNS_RESTRICT" == "1" ]] &&
       [[ "$CURRENT_RESTRICTION" != "1" ]]
    then
        echo "ERROR: Global AppArmor userns protection changed unexpectedly."
        exit 1
    fi
fi

echo

# ============================================================
# FINAL SELF-TEST
# ============================================================

echo "=== Final Bubblewrap Self-Test ==="

if bwrap \
    --ro-bind / / \
    --proc /proc \
    --dev /dev \
    --tmpfs /tmp \
    --unshare-pid \
    --new-session \
    /bin/true
then
    echo
    echo "Bubblewrap self-test PASSED."
else
    echo
    echo "ERROR: Bubblewrap self-test FAILED."
    echo
    echo "Lockdown has not been configured successfully."
    echo "No global user-namespace security setting was disabled."
    exit 1
fi

# ============================================================
# SUCCESS
# ============================================================

echo
echo "=============================================="
echo " Echo Adapt lockdown prerequisites are ready"
echo "=============================================="
echo
echo "Bubblewrap: $BWRAP_BIN"

if [[ -f "$APPARMOR_PROFILE" ]]; then
    echo "AppArmor profile: $APPARMOR_PROFILE"
fi

echo
echo "The global AppArmor user-namespace restriction was not"
echo "disabled by this setup."
echo
echo "Next step:"
echo "  ./run.sh --lockdown"
