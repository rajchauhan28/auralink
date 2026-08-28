#!/usr/bin/env bash
#
# Build AuraLink packages for Arch, Debian and Fedora.
#
# Every package is built inside that distribution's own container, so each
# binary links against the glibc it will actually run against. Building all
# three on the host would produce .deb and .rpm files carrying Arch's much
# newer glibc, which fail at startup on the target distro with
# "version `GLIBC_2.xx' not found".
#
# Usage: scripts/build-packages.sh [arch|deb|rpm|all]   (default: all)

set -Eeuo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

PKGNAME=auralink
VERSION="$(grep -m1 '^version' Cargo.toml | sed -E 's/version *= *"([^"]+)"/\1/')"
DIST="$REPO_ROOT/dist"
TARGET="${1:-all}"

echo "==> $PKGNAME $VERSION  (target: $TARGET)"

if [ -n "$(git status --porcelain)" ]; then
    echo "!!  Working tree is dirty. Packages are built from HEAD, so"
    echo "!!  uncommitted changes will NOT be included." >&2
fi

mkdir -p "$DIST" build
# Build from a clean HEAD archive. Never hand the working tree to Docker:
# target/ alone is several GB and would be copied into every container.
SRC_TAR="$REPO_ROOT/build/${PKGNAME}-${VERSION}.tar.gz"
git archive --format=tar.gz --prefix="${PKGNAME}-build/" -o "$SRC_TAR" HEAD
echo "==> source archive: $(du -h "$SRC_TAR" | cut -f1)"

# Rust in the distro repos is routinely too old for edition 2024, so every
# container installs the current stable toolchain via rustup.
RUSTUP='curl --proto =https --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable >/dev/null 2>&1; . "$HOME/.cargo/env"'

run_in() {   # run_in <image> <script>
    docker run --rm \
        -v "$SRC_TAR:/src.tar.gz:ro" \
        -v "$DIST:/out" \
        -e VERSION="$VERSION" \
        "$1" bash -euo pipefail -c "$2"
}

build_deb() {
    echo "==> Debian package (debian:bookworm)"
    run_in debian:bookworm "
        export DEBIAN_FRONTEND=noninteractive
        apt-get update -qq
        apt-get install -y -qq build-essential curl ca-certificates pkg-config \
            libfontconfig1-dev libxkbcommon-dev libwayland-dev libx11-dev \
            libxcursor-dev libxrandr-dev libxi-dev libgl1-mesa-dev >/dev/null
        $RUSTUP
        cargo install cargo-deb --locked >/dev/null 2>&1
        tar xzf /src.tar.gz -C /tmp
        cd /tmp/${PKGNAME}-build
        cargo deb --output /out/
        chmod 644 /out/*.deb
    "
}

build_rpm() {
    echo "==> Fedora package (fedora:latest)"
    run_in fedora:latest "
        dnf install -y -q gcc gcc-c++ curl pkgconf-pkg-config rpm-build \
            fontconfig-devel libxkbcommon-devel wayland-devel libX11-devel \
            libXcursor-devel libXrandr-devel libXi-devel mesa-libGL-devel >/dev/null
        $RUSTUP
        cargo install cargo-generate-rpm --locked >/dev/null 2>&1
        tar xzf /src.tar.gz -C /tmp
        cd /tmp/${PKGNAME}-build
        cargo build --release
        strip -s target/release/${PKGNAME} target/release/${PKGNAME}-bt || true
        cargo generate-rpm --output /out/
        chmod 644 /out/*.rpm
    "
}

build_arch() {
    echo "==> Arch package (archlinux:base-devel)"
    # makepkg refuses to run as root, so build as a dedicated unprivileged user.
    run_in archlinux:base-devel "
        pacman -Syu --noconfirm --needed base-devel git curl fontconfig \
            libxkbcommon wayland libx11 >/dev/null
        useradd -m builder
        cp /src.tar.gz /home/builder/${PKGNAME}-\$VERSION.tar.gz
        tar xzf /src.tar.gz -C /tmp
        cp /tmp/${PKGNAME}-build/PKGBUILD /home/builder/
        chown -R builder:builder /home/builder
        su builder -c '
            $RUSTUP
            cd /home/builder
            makepkg -f --noconfirm --skipinteg
        '
        cp /home/builder/*.pkg.tar.zst /out/
        chmod 644 /out/*.pkg.tar.zst
    "
}

case "$TARGET" in
    deb)  build_deb ;;
    rpm)  build_rpm ;;
    arch) build_arch ;;
    all)  build_arch; build_deb; build_rpm ;;
    *)    echo "Unknown target: $TARGET (use arch|deb|rpm|all)" >&2; exit 1 ;;
esac

echo
echo "==> Packages in dist/"
ls -lh "$DIST"
