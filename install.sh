#!/bin/bash

# Create local bin if it doesn't exist
mkdir -p "$HOME/.local/bin"

# Install binaries
echo "Installing binaries to $HOME/.local/bin/..."
cp target/release/auralink "$HOME/.local/bin/"
cp target/release/auralink-bt "$HOME/.local/bin/"
chmod +x "$HOME/.local/bin/auralink"
chmod +x "$HOME/.local/bin/auralink-bt"

# Install desktop entries
echo "Installing desktop entries to $HOME/.local/share/applications/..."
mkdir -p "$HOME/.local/share/applications"
cp auralink.desktop "$HOME/.local/share/applications/"
cp auralink-bt.desktop "$HOME/.local/share/applications/"

# Update desktop database
if command -v update-desktop-database &> /dev/null; then
    update-desktop-database "$HOME/.local/share/applications"
fi

echo "Installation complete! You can now find 'AuraLink' and 'AuraLink Bluetooth' in your application menu."
echo "Note: Ensure $HOME/.local/bin is in your PATH."
