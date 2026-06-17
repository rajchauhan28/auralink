#!/bin/bash
set -e

echo "Starting build process for AuraLink..."

# Derive the version from Cargo.toml so every artifact stays in sync.
VERSION=$(grep -m1 '^version' Cargo.toml | sed -E 's/version *= *"([^"]+)"/\1/')
echo "Version: ${VERSION}"

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
# The PKGBUILD expects a source tarball named auralink-<ver>.tar.gz that
# extracts into a top-level auralink-build/ directory. Generate it from the
# committed tree so the package is reproducible and the script is self-contained.
SRC_TARBALL="auralink-${VERSION}.tar.gz"
echo "Generating source tarball ${SRC_TARBALL}..."
git archive --format=tar.gz --prefix="auralink-build/" -o "${SRC_TARBALL}" HEAD

# Run makepkg in an isolated directory. makepkg uses ./src and ./pkg as its
# build/staging dirs; running it in the repo root would collide with the
# project's own src/ directory, so build in a throwaway dir instead.
MAKEPKG_DIR=$(mktemp -d)
cp PKGBUILD "${SRC_TARBALL}" "${MAKEPKG_DIR}/"
( cd "${MAKEPKG_DIR}" && makepkg -f --noconfirm )
mv "${MAKEPKG_DIR}"/auralink-*.pkg.tar.zst output/
rm -rf "${MAKEPKG_DIR}"

# Keep a copy of the source tarball alongside the packages.
mv "${SRC_TARBALL}" output/

echo "---------------------------------------"
echo "Build complete! Packages are in output/"
ls -lh output/
echo "---------------------------------------"
