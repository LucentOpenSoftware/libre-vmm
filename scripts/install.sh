#!/usr/bin/env bash
# Install Libre VMM binaries and desktop entry.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
BIN_DIR="$HOME/.local/bin"
APP_DIR="$HOME/.local/share/applications"
ICON_DIR="$HOME/.local/share/icons/hicolor/256x256/apps"
DATA_DIR="$HOME/.local/share/libre-vmm"

echo "[*] Installing Libre VMM..."

# Ensure directories exist
mkdir -p "$BIN_DIR" "$APP_DIR" "$ICON_DIR" "$DATA_DIR/disks" "$DATA_DIR/configs"

# Build if not built
if [[ ! -f "$PROJECT_DIR/target/release/vmm-gui" ]]; then
    echo "[*] Building release binaries..."
    cd "$PROJECT_DIR"
    cargo build --release
fi

# Copy binaries
cp "$PROJECT_DIR/target/release/vmm-gui" "$BIN_DIR/libre-vmm"
cp "$PROJECT_DIR/target/release/vmm-cli" "$BIN_DIR/vmm"
chmod +x "$BIN_DIR/libre-vmm" "$BIN_DIR/vmm"

echo "[*] Installed binaries to $BIN_DIR/"

# Install shell completions
echo "[*] Installing shell completions..."
COMPLETIONS_BASH="$HOME/.local/share/bash-completion/completions"
COMPLETIONS_ZSH="$HOME/.local/share/zsh/site-functions"
COMPLETIONS_FISH="$HOME/.config/fish/completions"
mkdir -p "$COMPLETIONS_BASH" "$COMPLETIONS_ZSH" "$COMPLETIONS_FISH"
"$BIN_DIR/vmm" completions bash > "$COMPLETIONS_BASH/vmm" 2>/dev/null && echo "    bash: $COMPLETIONS_BASH/vmm"
"$BIN_DIR/vmm" completions zsh > "$COMPLETIONS_ZSH/_vmm" 2>/dev/null && echo "    zsh:  $COMPLETIONS_ZSH/_vmm"
"$BIN_DIR/vmm" completions fish > "$COMPLETIONS_FISH/vmm.fish" 2>/dev/null && echo "    fish: $COMPLETIONS_FISH/vmm.fish"

# Create desktop entry
cat > "$APP_DIR/libre-vmm.desktop" << 'DESKTOP'
[Desktop Entry]
Name=Libre VMM
Comment=Virtual Machine Manager — Create and run virtual machines
Exec=libre-vmm
Icon=libre-vmm
Terminal=false
Type=Application
Categories=System;Emulator;
Keywords=virtual;machine;vm;qemu;kvm;virtualization;
StartupWMClass=libre-vmm
DESKTOP

echo "[*] Created desktop entry"

# Ensure PATH includes ~/.local/bin
if ! echo "$PATH" | grep -q "$HOME/.local/bin"; then
    echo ""
    echo "[!] Add $HOME/.local/bin to your PATH:"
    echo '    export PATH="$HOME/.local/bin:$PATH"'
    echo "    (add this to your ~/.bashrc or ~/.zshrc)"
fi

echo ""
echo "[*] Installation complete!"
echo "    GUI:  libre-vmm"
echo "    CLI:  vmm list | vmm create | vmm start | vmm console"
echo ""
