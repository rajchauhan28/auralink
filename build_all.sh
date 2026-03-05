#!/bin/bash
set -e

echo "Starting build process for AuraLink..."

# 1. Create output directory
mkdir -p output
rm -f output/*

# 2. Build release binary
echo "Building release binary..."
cargo build --release

# 3. Build AppImage
echo "Building AppImage..."
rm -rf AppDir
NO_STRIP=1 ./linuxdeploy --executable target/release/auralink \
    --desktop-file auralink.desktop \
    --icon-file assets/auralink.svg \
    --appdir AppDir \
    --output appimage

mv AuraLink-x86_64.AppImage output/

# 4. Build DEB package
echo "Building DEB package..."
cargo deb
mv target/debian/auralink_*.deb output/

# 5. Build Arch Linux package
echo "Building Arch Linux package..."
# makepkg usually builds in the current directory, then we move it
# -f forces overwrite, -s installs dependencies
makepkg -f --noconfirm
mv auralink-*.pkg.tar.zst output/

echo "---------------------------------------"
echo "Build complete! Packages are in output/"
ls -lh output/
echo "---------------------------------------"
