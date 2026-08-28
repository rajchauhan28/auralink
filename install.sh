#!/usr/bin/env bash
#
# Install AuraLink into the current user's ~/.local.
#
# `set -e` matters here: the previous version ignored every failure. Upgrading
# while the auto-connect daemon was running made `cp` fail with
# "Text file busy" -- you cannot overwrite a running executable -- and the
# script carried on to print "Installation complete!" while leaving the OLD
# binary in place. That is a silent no-op upgrade, and it is why a rebuilt
# daemon could appear to change nothing at all.

set -Eeuo pipefail

BIN_DIR="$HOME/.local/bin"
APP_DIR="$HOME/.local/share/applications"
UNIT_DIR="$HOME/.config/systemd/user"
UNIT="auralink-bt-daemon.service"

# Pick the freshest build rather than assuming release exists.
if [ -x "target/release/auralink-bt" ]; then
    BUILD_DIR="target/release"
elif [ -x "target/debug/auralink-bt" ]; then
    BUILD_DIR="target/debug"
else
    echo "Error: no built binaries found. Run 'cargo build --release' first." >&2
    exit 1
fi
echo "Installing from $BUILD_DIR/"

mkdir -p "$BIN_DIR" "$APP_DIR" "$UNIT_DIR"

# Stop the daemon before replacing the binary it is executing.
daemon_was_running=0
if command -v systemctl &>/dev/null && systemctl --user is-active --quiet "$UNIT"; then
    daemon_was_running=1
    echo "Stopping $UNIT to replace its binary..."
    systemctl --user stop "$UNIT"
fi

echo "Installing binaries to $BIN_DIR/..."
for binary in auralink auralink-bt; do
    # Unlink first: removing a busy executable is allowed even when
    # overwriting it in place is not, and it also replaces any symlink a
    # dotfiles stow may have left at this path.
    rm -f "$BIN_DIR/$binary"
    cp "$BUILD_DIR/$binary" "$BIN_DIR/$binary"
    chmod +x "$BIN_DIR/$binary"
done

echo "Installing desktop entries to $APP_DIR/..."
cp auralink.desktop auralink-bt.desktop "$APP_DIR/"
chmod 644 "$APP_DIR/auralink.desktop" "$APP_DIR/auralink-bt.desktop"

echo "Installing the Bluetooth auto-connect daemon service..."
# The committed unit points at /usr/bin so the distro packages work. A
# source install puts the binary in ~/.local/bin instead, so repoint it.
sed 's|^ExecStart=/usr/bin/auralink-bt|ExecStart=%h/.local/bin/auralink-bt|' \
    "$UNIT" > "$UNIT_DIR/$UNIT"
# This repository may live on a filesystem that forces mode 777 (an NTFS
# mount, say) and `cp` carries that across; systemd rejects a world-writable
# unit file.
chmod 644 "$UNIT_DIR/$UNIT"

if command -v systemctl &>/dev/null; then
    systemctl --user daemon-reload
    if systemctl --user enable --now "$UNIT"; then
        echo "Bluetooth auto-connect daemon enabled and running."
    else
        echo "Warning: could not start $UNIT; check 'systemctl --user status $UNIT'." >&2
    fi
elif [ "$daemon_was_running" = 1 ]; then
    echo "Warning: systemctl unavailable; restart the daemon manually." >&2
fi

if command -v update-desktop-database &>/dev/null; then
    update-desktop-database "$APP_DIR"
fi

echo
echo "Installation complete. 'AuraLink' and 'AuraLink Bluetooth' are in your application menu."
echo "Note: ensure $BIN_DIR is in your PATH."
