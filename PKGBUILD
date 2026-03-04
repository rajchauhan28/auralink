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
  # Use a hidden build directory to avoid collision with 'src' or 'pkg'
  # $startdir is the directory where the PKGBUILD is located
  cd "$startdir"
  cargo build --release
}

package() {
  install -Dm755 "$startdir/target/release/auralink" "${pkgdir}/usr/bin/auralink"
  install -Dm644 "$startdir/auralink.desktop" "${pkgdir}/usr/share/applications/auralink.desktop"
  install -Dm644 "$startdir/assets/auralink.svg" "${pkgdir}/usr/share/icons/hicolor/scalable/apps/auralink.svg"
}
