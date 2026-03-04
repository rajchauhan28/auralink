# AuraLink 🌌

**AuraLink** is a modern, lightweight, and aesthetically pleasing Wi-Fi and VPN manager for Linux. Built with **Rust** and **Slint**, it offers a seamless user experience with live theme synchronization.

<p align="center">
  <img src="https://github.com/user-attachments/assets/e5f516bd-44ab-41da-870e-7ef51ad5a59b" width="300" />
  <img src="https://github.com/user-attachments/assets/de0158ba-13ee-4f9b-9e16-315d2bd7292b" width="300" />
</p>

## ✨ Features

- **Live Pywal Sync 🎨**: Automatically updates application colors when your wallpaper changes.
- **Modern UI 💎**: A clean, intuitive interface with smooth animations and rounded aesthetics.
- **Smart Connection Management ⚡**: Connect, disconnect, and monitor signal strength.
- **Network Stats & Graphs 📊**: Real-time speed monitoring with live graphs.
- **Advanced Network Options ⚙️**: Configure auto-connect, priorities, and custom DNS.
- **Context-Aware Actions 󰇘**: Three-dot menu for "Forget", "Info", and "Advanced" options.
- **VPN Support 🔒**: Manage VPN connections (Wireguard, OpenVPN, WARP, etc).

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
sudo dpkg -i auralink_0.1.0_amd64.deb
sudo apt-get install -f
```

### AppImage (Universal)
Download the `AuraLink-x86_64.AppImage` from the [releases page](https://github.com/rajchauhan28/auralink/releases).
```bash
chmod +x AuraLink-x86_64.AppImage
./AuraLink-x86_64.AppImage
```

## 🛠 Tech Stack
- **Rust**: Logic & Performance.
- **Slint**: UI Framework (using winit backend).
- **NetworkManager**: Backend integration.
- **Pywal**: Theme orchestration.

## 📝 License
MIT License - Copyright (c) 2026 rajchauhan28
