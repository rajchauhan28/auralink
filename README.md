# AuraLink 🌌: Modern Linux Networking Manager

**AuraLink** is a blazing-fast, aesthetic, and customizable Wi-Fi, VPN, and Bluetooth manager for Linux. Engineered with **Rust** and **Slint**, it delivers a seamless, high-performance user experience with live Pywal theme synchronization.

> "Networking doesn't have to be ugly."

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Language-Rust-orange.svg)](https://www.rust-lang.org/)
[![Slint](https://img.shields.io/badge/UI-Slint-blue.svg)](https://slint.dev/)

<p align="center">
  <img width="855" height="649" alt="screenshot_20260413_221455" src="https://github.com/user-attachments/assets/43ba681e-ee20-4cd7-ac86-2d001155d08e" />
  <img width="834" height="631" alt="image" src="https://github.com/user-attachments/assets/9dd10dac-4905-4927-8c29-a79082f2f9dd" />
</p>

## ✨ Features

- **Live Pywal Sync 🎨**: Automatically updates application colors when your wallpaper changes.
- **Modern UI 💎**: A clean, intuitive interface with smooth animations and rounded aesthetics.
- **Smart Connection Management ⚡**: Connect, disconnect, and monitor signal strength.
- **Network Stats & Graphs 📊**: Real-time speed monitoring with live graphs.
- **Advanced Network Options ⚙️**: 
  - **MAC Spoofing**: Custom cloned MAC addresses.
  - **IP Config**: Full IPv4 and IPv6 manual/auto configuration.
  - **Password Management**: Easily update Wi-Fi passwords.
  - **Connection Control**: Configure auto-connect and priorities.
- **Context-Aware Actions 󰇘**: Three-dot menu for "Forget", "Info", and "Advanced" options.
- **VPN Support 🔒**: Manage VPN connections (Wireguard, OpenVPN, WARP, etc).
- **Floating Window 🪟**: Default floating behavior with resizability.

## 📦 Installation

Prebuilt packages for Arch, Debian and Fedora are attached to every
[release](https://github.com/rajchauhan28/auralink/releases). Each one is built
inside that distribution's own container, so it links against the right glibc.

### Arch Linux
```bash
sudo pacman -U auralink-*-x86_64.pkg.tar.zst
```

Or build from the tree:
```bash
git clone https://github.com/rajchauhan28/auralink.git
cd auralink
makepkg -si
```

### Debian / Ubuntu
```bash
sudo apt install ./auralink_*_amd64.deb
```

### Fedora
```bash
sudo dnf install ./auralink-*.x86_64.rpm
```

### From source
```bash
git clone https://github.com/rajchauhan28/auralink.git
cd auralink
cargo build --release
./install.sh
```

`install.sh` installs into `~/.local` and enables the Bluetooth auto-connect
daemon, repointing the systemd unit at `~/.local/bin` instead of the
`/usr/bin` path the packages use.

### Bluetooth auto-connect

Every install method ships `auralink-bt-daemon.service`, which reconnects
trusted devices as they come back into range. Packages install it to
`/usr/lib/systemd/user`; enable it per user:

```bash
systemctl --user enable --now auralink-bt-daemon.service
```

### Building the packages yourself

```bash
./scripts/build-packages.sh all      # or: arch | deb | rpm
```

Requires Docker. Packages land in `dist/`.

## 🛠 Building from source

You can use the provided build script to generate all package formats (AppImage, DEB, Arch):
```bash
./build_all.sh
```
The output files will be located in the `output/` directory.
## File structure
```
├── .gitignore
├── AppDir/ (Standalone LinuxApp directory with binaries, libs, icons, and desktop files)
├── assets/
│   └── auralink.svg
├── auralink-0.1.0.tar.gz & auralink_0.1.0_amd64.deb (Pre-built archives)
├── auralink-bt.desktop & auralink.desktop
├── build_all.sh & install.sh
├── build.rs
├── Cargo.lock & Cargo.toml
├── docs/ (Design specs and implementation plans)
├── linuxdeploy (AppImage bundler binary)
├── pkg/ (Compiled Arch Linux packages with debug symbols)
├── PKGBUILD
├── reproduce_bt_cmds.sh
├── src/
│   ├── bt_backend.rs
│   ├── bt_main.rs
│   ├── main.rs
│   └── nm_backend.rs
├── test.slint & test_slint.rs
└── ui/
    ├── bluetooth.slint
    └── wifi.slint
```
## 📝 License
MIT License - Copyright (c) 2026 rajchauhan28
