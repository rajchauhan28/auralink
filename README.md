# AuraLink 🌌: Modern Linux Networking Manager

**AuraLink** is a blazing-fast, aesthetic, and customizable Wi-Fi, VPN, and Bluetooth manager for Linux. Engineered with **Rust** and **Slint**, it delivers a seamless, high-performance user experience with live Pywal theme synchronization.

> "Networking doesn't have to be ugly."

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Language-Rust-orange.svg)](https://www.rust-lang.org/)
[![Slint](https://img.shields.io/badge/UI-Slint-blue.svg)](https://slint.dev/)

<p align="center">
  <img src="https://github.com/user-attachments/assets/e5f516bd-44ab-41da-870e-7ef51ad5a59b" width="300" />
  <img src="https://github.com/user-attachments/assets/de0158ba-13ee-4f9b-9e16-315d2bd7292b" width="300" />
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

### Arch Linux
Clone the repo and build using `makepkg`:
```bash
git clone https://github.com/rajchauhan28/auralink.git
cd auralink
makepkg -si
```

### Debian / Ubuntu
Download the `.deb` from the [releases page](https://github.com/rajchauhan28/auralink/releases) and install:
```bash
sudo dpkg -i auralink_0.1.4_amd64.deb
sudo apt-get install -f
```

### AppImage (Universal)
Download the `AuraLink-x86_64.AppImage` from the [releases page](https://github.com/rajchauhan28/auralink/releases).
```bash
chmod +x AuraLink-x86_64.AppImage
./AuraLink-x86_64.AppImage
```

## 🛠 Building from source

You can use the provided build script to generate all package formats (AppImage, DEB, Arch):
```bash
./build_all.sh
```
The output files will be located in the `output/` directory.

## 📝 License
MIT License - Copyright (c) 2026 rajchauhan28
