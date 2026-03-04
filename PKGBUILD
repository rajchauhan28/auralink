# Maintainer: rajchauhan28
pkgname=auralink
pkgver=0.1.0
pkgrel=1
pkgdesc="A modern, lightweight, and aesthetic Wi-Fi and VPN manager for Linux."
arch=('x86_64')
url="https://github.com/rajchauhan28/auralink"
license=('MIT')
depends=('gcc-libs' 'glibc' 'networkmanager' 'qt6-base')
makedepends=('cargo')
source=()

build() {
  # Create a separate build directory to avoid conflicts
  mkdir -p "$srcdir/build_dir"
  cp -r "$startdir/src" "$startdir/ui" "$startdir/Cargo.toml" "$startdir/Cargo.lock" "$startdir/build.rs" "$srcdir/build_dir/"
  cd "$srcdir/build_dir"
  cargo build --release
}

package() {
  cd "$srcdir/build_dir"
  install -Dm755 "target/release/auralink" "${pkgdir}/usr/bin/auralink"
  install -Dm644 "$startdir/auralink.desktop" "${pkgdir}/usr/share/applications/auralink.desktop"
  install -Dm644 "$startdir/assets/auralink.svg" "${pkgdir}/usr/share/icons/hicolor/scalable/apps/auralink.svg"
}
