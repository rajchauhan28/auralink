# Maintainer: rajchauhan28
pkgname=auralink
pkgver=0.1.7
pkgrel=1
pkgdesc="A blazing-fast, aesthetic Wi-Fi, VPN, and Bluetooth manager for Linux. Built with Rust and Slint, featuring live Pywal sync."
arch=('x86_64')
url="https://github.com/rajchauhan28/auralink"
license=('MIT')
# Derived from `ldd` on the release binaries. Slint is statically linked, so
# there is no slint runtime package to depend on.
depends=('gcc-libs' 'glibc' 'fontconfig' 'freetype2' 'libpng' 'zlib'
         'networkmanager')
optdepends=('bluez-utils: Bluetooth management and auto-connect'
            'libnotify: desktop notifications'
            'python-pywal: live colour scheme sync')
makedepends=('rust')
# The release profile already strips, so makepkg's split debug package
# comes out empty; suppress it rather than publish a 5 KB stub.
options=('!debug')
source=("auralink-${pkgver}.tar.gz")
sha256sums=('SKIP')

build() {
  cd "$srcdir/auralink-build"
  export RUSTUP_TOOLCHAIN=stable
  export CARGO_TARGET_DIR=target
  cargo build --release --locked
}

package() {
  cd "$srcdir/auralink-build"
  install -Dm755 "target/release/auralink"    "${pkgdir}/usr/bin/auralink"
  install -Dm755 "target/release/auralink-bt" "${pkgdir}/usr/bin/auralink-bt"
  install -Dm644 "auralink.desktop"    "${pkgdir}/usr/share/applications/auralink.desktop"
  install -Dm644 "auralink-bt.desktop" "${pkgdir}/usr/share/applications/auralink-bt.desktop"
  install -Dm644 "assets/auralink.svg" "${pkgdir}/usr/share/icons/hicolor/scalable/apps/auralink.svg"
  # Shipped so a packaged install gets the auto-connect daemon that
  # install.sh sets up for a from-source install.
  install -Dm644 "auralink-bt-daemon.service" \
    "${pkgdir}/usr/lib/systemd/user/auralink-bt-daemon.service"
  install -Dm644 "LICENSE" "${pkgdir}/usr/share/licenses/${pkgname}/LICENSE"
  install -Dm644 "README.md" "${pkgdir}/usr/share/doc/${pkgname}/README.md"
}
