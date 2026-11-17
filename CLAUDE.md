# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

AuraLink: a Linux Wi-Fi/VPN GUI (`auralink`) and Bluetooth GUI + auto-connect daemon (`auralink-bt`), written in Rust (edition 2024) with a Slint UI. Both binaries also work as one-shot CLIs (`status`, `fullstatus`, `waybar-stream`, `connect`, …) for status bars and scripts.

## Commands

```bash
cargo build --release                         # both binaries -> target/release/{auralink,auralink-bt}
cargo run --bin auralink                      # Wi-Fi GUI (no args = GUI; `popup`/`gui`/`open` too)
cargo run --bin auralink-bt -- status         # any CLI subcommand; `--help` lists them
cargo run --bin auralink-bt -- daemon         # headless auto-connect loop (what the systemd unit runs)

cargo test --locked                           # all tests (only src/bt_main.rs has tests)
cargo test --bin auralink-bt test_handle_commands_help   # single test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check

./install.sh                                  # install built binaries into ~/.local and (re)start the BT daemon unit
./scripts/build-packages.sh [arch|deb|rpm|all]   # distro packages, each built in its own Docker container -> dist/
```

- CI (`.github/workflows/ci.yml`) builds and tests with `--locked`, so `Cargo.lock` must be committed in sync with any `Cargo.toml` change. fmt and clippy run in CI but are `continue-on-error`.
- The `status`/`fullstatus` tests call `handle_commands`, which really shells out to `bluetoothctl` on the host; they assert only that a command was handled.
- `build_all.sh` is the older host-side build (AppImage via the untracked `./linuxdeploy`, plus deb and Arch, into `output/`). `scripts/build-packages.sh` builds each package in its own distro's container so it links against that distro's glibc.

## Architecture

There is no library crate. `src/main.rs` (`auralink`) and `src/bt_main.rs` (`auralink-bt`) are separate binaries that each declare their backend modules with `mod`:

- `main.rs` -> `nm_backend.rs` (NetworkManager/VPN) **and** `bt_backend.rs` (used only for airplane mode). Because `bt_backend` is compiled into both binaries it carries `#![allow(dead_code)]`.
- `bt_main.rs` -> `bt_backend.rs`.
- The pywal/config/notification helpers (`AppConfig`, `parse_hex`, `apply_pywal_theme`, `send_notification`) are **duplicated** in both binaries, not shared. A fix to one usually needs the same fix in the other. Config files differ: `~/.config/auralink/config.json` vs `bt_config.json`.

**Backends are subprocess wrappers, not D-Bus clients.** `nm_backend` drives `nmcli` (terse `-t` output, parsed with `parse_nmcli_line`, which honours `\:` escapes), plus `systemctl`/`warp-cli` for WARP, `ip link`/`pgrep` for Hiddify, `ping`, and `/proc/net/dev` for throughput. `bt_backend` drives `bluetoothctl`, `busctl`/`dbus-send` (BlueZ object tree, trusted devices, per-component batteries), and `pactl` (audio card profiles). All calls block.

**UI wiring.** `build.rs` compiles `ui/wifi.slint` and `ui/bluetooth.slint` into `OUT_DIR`, and each binary `include!`s its generated module. A new `.slint` file must be added to `build.rs` (both the `compile` and the `rerun-if-changed` lines). Each binary's GUI follows the same pattern:
- Slint callbacks (`on_connect`, `on_toggle_vpn`, …) spawn a `std::thread`, call the blocking backend, then push results back with `slint::invoke_from_event_loop` using a `Weak` window handle. Never call a backend function on the UI thread.
- Long-lived background threads poll and repaint: in `auralink` these are the network/VPN list (10s, or sooner after `refresh` sets `force_refresh`), throughput graph (200ms), and pywal `~/.cache/wal/colors.json` mtime watch (2s). In `auralink-bt` they are the device list (5s) and the auto-connect loop.
- Theme colours live in a Slint `global` (`WifiPalette` in wifi.slint, `Palette` in bluetooth.slint), set from Rust when pywal is enabled and reset to hardcoded defaults when it is disabled.
- `main.rs` is `#[tokio::main]`, but all concurrency is `std::thread`; tokio is not actually used.

**CLI dispatch** is a hand-written `match` on `args[1]` (in `main()` for `auralink`, in `handle_commands()` for `auralink-bt`). The usage text is duplicated in the `--help` arm and the unknown-command arm, so a new subcommand needs both updated.

## Bluetooth invariants (learned from bugs — keep them)

- **Always go through `bluetoothctl(timeout, args)`** for anything that pages a device (connect/pair/trust/remove/power/scan). A bare `bluetoothctl connect <mac>` to an out-of-range device never returns and wedged the daemon loop.
- **`bluetoothctl` exits 0 even on failure.** Judge success with `bluetoothctl_ok(output, success_marker)` (text match), never `status.success()`. The daemon also re-checks `get_connected_address()` after a connect rather than trusting the return value.
- **Discovery is a tracked child** in `SCAN_CHILD`; use `start_scan`/`stop_scan`/`scan_running` (which kill and reap it). Don't `pkill` by pattern, and don't drop the `Child` handle, because that leaks zombies.
- **Never run discovery while a device is connected**, because it makes A2DP playback stutter.
- **Auto-connect exists twice:** `run_daemon()` (headless, run by `auralink-bt-daemon.service`) and a loop inside the `auralink-bt` GUI. The GUI loop stands down while `systemctl --user is-active auralink-bt-daemon.service` succeeds, and checks this on every iteration. Behaviour changes usually belong in both loops.
- Don't hardcode `hci0`; `list_adapters()` enumerates adapters, which can renumber after suspend.
- `bluez-utils` is optional at runtime: a missing `bluetoothctl` must yield empty results, not a panic.
- After connecting, `force_a2dp_profile` picks the highest-priority available `a2dp-sink*` profile from `pactl list cards`.

## Packaging and release

- The version lives in **both** `Cargo.toml` and `PKGBUILD` (`pkgver`); bump them together and refresh `Cargo.lock`.
- A file installed onto the system has to be listed in four places: `PKGBUILD` `package()`, `[package.metadata.deb].assets`, `[package.metadata.generate-rpm].assets` in `Cargo.toml`, and `install.sh`.
- `auralink-bt-daemon.service` is a **user** unit with `ExecStart=/usr/bin/auralink-bt daemon` for the packages. `install.sh` rewrites it to `%h/.local/bin` and stops the running daemon before replacing the binary, because overwriting a running executable fails with "Text file busy".
- Packages are built from `git archive HEAD` (prefix `auralink-build/`), so uncommitted changes are not included.
- Pushing a `v*` tag runs `.github/workflows/release.yml`, which builds deb (bookworm), rpm (fedora), and Arch (`archlinux:base-devel`, as a non-root user) in containers and publishes a GitHub release.
- The repo lives on a filesystem that reports every file as mode 777. `install.sh` `chmod 644`s the unit because systemd rejects a world-writable one.

## Untracked clutter in the working tree

`AppDir/`, `squashfs-root/`, `pkg/`, `auralink/` (a stray bare git repo), `output/`, `dist/`, `build/`, `linuxdeploy`, `*.tar.gz`, and `test_slint*`/`test.slint` are gitignored build or scratch artifacts, not source. Don't edit them or search them for code. `docs/superpowers/` holds the design spec and implementation plan for the `status`/`fullstatus` commands.
