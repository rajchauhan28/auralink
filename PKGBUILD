# Maintainer: rajchauhan28
pkgname=auralink
pkgver=0.1.4
pkgrel=1
pkgdesc="A modern, lightweight, and aesthetic Wi-Fi and VPN manager for Linux."
arch=('x86_64')
url="https://github.com/rajchauhan28/auralink"
license=('MIT')
depends=('gcc-libs' 'glibc' 'networkmanager')
makedepends=('cargo')
source=("${pkgname}::git+file://${startdir}")
sha256sums=('SKIP')

build() {
  cd "$srcdir/${pkgname}"
  cargo build --release
}

package() {
  cd "$srcdir/${pkgname}"
  install -Dm755 "target/release/auralink" "${pkgdir}/usr/bin/auralink"
  install -Dm644 "auralink.desktop" "${pkgdir}/usr/share/applications/auralink.desktop"
  install -Dm644 "assets/auralink.svg" "${pkgdir}/usr/share/icons/hicolor/scalable/apps/auralink.svg"
}
