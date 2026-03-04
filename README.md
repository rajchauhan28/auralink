# AuraLink 🌌

**AuraLink** is a modern, lightweight, and aesthetically pleasing Wi-Fi and VPN manager for Linux. Built with **Rust** and **Slint**, it offers a seamless user experience with live theme synchronization.

![AuraLink Banner](https://raw.githubusercontent.com/rajchauhan28/auralink/main/assets/banner.png) *(Note: Placeholder link)*

## ✨ Features

- **Live Pywal Sync 🎨**: Automatically updates application colors when your wallpaper/theme changes via Pywal.
- **Modern UI 💎**: A clean, intuitive interface with smooth animations and rounded aesthetics.
- **Smart Connection Management ⚡**: Connect, disconnect, and monitor signal strength and security protocols.
- **Network Stats & Graphs 📊**: Real-time download/upload speed monitoring with live graphs.
- **Advanced Network Options ⚙️**: Configure auto-connect, priorities, and custom DNS directly within the app.
- **Context-Aware Actions 󰇘**: Right-click or 3-dot menu for "Forget", "Info", and "Advanced" options.
- **VPN Support 🔒**: Manage VPN connections and proxies, including Cloudflare WARP and Hiddify.
- **Speedy Performance 🚀**: Built in Rust for maximum performance and minimal resource usage.

## 📦 Installation

### AppImage (Recommended)
Download the latest `AuraLink-x86_64.AppImage` from the [releases page](https://github.com/rajchauhan28/auralink/releases).
```bash
chmod +x AuraLink-x86_64.AppImage
./AuraLink-x86_64.AppImage
```

### Build from Source
Ensure you have Rust and Slint installed.
```bash
git clone https://github.com/rajchauhan28/auralink.git
cd auralink
cargo build --release
```

## 🛠 Tech Stack
- **Rust**: Core logic and backend.
- **Slint**: High-performance UI framework.
- **NetworkManager (nmcli)**: System-level network interaction.
- **Pywal**: For dynamic theme orchestration.

## 📝 License
MIT License - Copyright (c) 2026

---
*Created with ❤️ byrajchauhan28*
