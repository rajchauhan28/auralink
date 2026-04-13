# AuraLink 🌌

**AuraLink** is a modern, lightweight, and aesthetically pleasing Wi-Fi and VPN manager for Linux. Built with **Rust** and **Slint**, it offers a seamless user experience with live theme synchronization.

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
