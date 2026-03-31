# Maintainer: rajchauhan28
pkgname=auralink
pkgver=0.1.5
pkgrel=1
pkgdesc="A modern, lightweight, and aesthetic Wi-Fi and VPN manager for Linux."
arch=('x86_64')
url="https://github.com/rajchauhan28/auralink"
license=('MIT')
depends=('gcc-libs' 'glibc' 'networkmanager')
makedepends=('cargo')
source=()

build() {
  cd "$startdir"
  cargo build --release
}

package() {
  cd "$startdir"
  install -Dm755 "target/release/auralink" "${pkgdir}/usr/bin/auralink"
  install -Dm755 "target/release/auralink-bt" "${pkgdir}/usr/bin/auralink-bt"
  install -Dm644 "auralink.desktop" "${pkgdir}/usr/share/applications/auralink.desktop"
  install -Dm644 "auralink-bt.desktop" "${pkgdir}/usr/share/applications/auralink-bt.desktop"
  install -Dm644 "assets/auralink.svg" "${pkgdir}/usr/share/icons/hicolor/scalable/apps/auralink.svg"
}
