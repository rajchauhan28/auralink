# Maintainer: rajchauhan28
pkgname=auralink
pkgver=0.1.6
pkgrel=1
pkgdesc="A blazing-fast, aesthetic Wi-Fi, VPN, and Bluetooth manager for Linux. Built with Rust and Slint, featuring live Pywal sync."
arch=('x86_64')
url="https://github.com/rajchauhan28/auralink"
license=('MIT')
depends=('gcc-libs' 'glibc' 'networkmanager')
makedepends=('cargo' 'rust')
source=("auralink-${pkgver}.tar.gz")
sha256sums=('SKIP')

build() {
  cd "$srcdir/auralink-build"
  cargo build --release
}

package() {
  cd "$srcdir/auralink-build"
  install -Dm755 "target/release/auralink" "${pkgdir}/usr/bin/auralink"
  install -Dm755 "target/release/auralink-bt" "${pkgdir}/usr/bin/auralink-bt"
  install -Dm644 "auralink.desktop" "${pkgdir}/usr/share/applications/auralink.desktop"
  install -Dm644 "auralink-bt.desktop" "${pkgdir}/usr/share/applications/auralink-bt.desktop"
  install -Dm644 "assets/auralink.svg" "${pkgdir}/usr/share/icons/hicolor/scalable/apps/auralink.svg"
}
