#!/bin/bash

# Create local bin if it doesn't exist
mkdir -p "$HOME/.local/bin"

# Install binary
echo "Installing binary to $HOME/.local/bin/wifi-manager..."
cp target/release/wifi-manager "$HOME/.local/bin/"
chmod +x "$HOME/.local/bin/wifi-manager"

# Install desktop entry
echo "Installing desktop entry to $HOME/.local/share/applications/wifi-manager.desktop..."
mkdir -p "$HOME/.local/share/applications"
cp wifi-manager.desktop "$HOME/.local/share/applications/"

# Update desktop database
if command -v update-desktop-database &> /dev/null; then
    update-desktop-database "$HOME/.local/share/applications"
fi

echo "Installation complete! You can now find 'Wi-Fi Manager' in your application menu."
echo "Note: Ensure $HOME/.local/bin is in your PATH."
