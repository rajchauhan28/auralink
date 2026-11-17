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

cargo test --locked                           # unit tests for both binaries
cargo test --bin auralink-bt help_succeeds    # single test (name filter)
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check

./install.sh                                  # install built binaries into ~/.local and (re)start the BT daemon unit
./scripts/build-packages.sh [arch|deb|rpm|all]   # distro packages, each built in its own Docker container -> dist/
```

- CI (`.github/workflows/ci.yml`) builds and tests with `--locked`, so `Cargo.lock` must be committed in sync with any `Cargo.toml` change. fmt and clippy run in CI as `continue-on-error`, but the tree is currently clean under both (`clippy -D warnings`); keep it that way.
- Shared modules (`common.rs`, `bt_backend.rs`) compile into both test binaries, so their tests run twice.
- The `status`/`fullstatus` CLI tests read live BlueZ state through `busctl`. Never add tests that connect, disconnect or power real devices. Parsing is tested against JSON fixtures captured from BlueZ 5.88 and PipeWire 1.6.
- `build_all.sh` is the older host-side build (AppImage via the untracked `./linuxdeploy`, plus deb and Arch, into `output/`). `scripts/build-packages.sh` builds each package in its own distro's container so it links against that distro's glibc.

## Architecture

There is no library crate. `src/main.rs` (`auralink`) and `src/bt_main.rs` (`auralink-bt`) are separate binaries that each declare the modules they use with `mod`:

- `main.rs` -> `nm_backend.rs`, `common.rs`, and `bt_backend.rs` (only for airplane mode).
- `bt_main.rs` -> `bt_backend.rs`, `autoconnect.rs`, `common.rs`.
- `common.rs` holds what both windows share: the per-app config file (`~/.config/auralink/config.json` vs `bt_config.json`), `send_notification`, and the pywal `Theme` and its live watcher (`spawn_pywal_watch`). Each binary maps `Theme` onto its own Slint global (`WifiPalette` in wifi.slint, `Palette` in bluetooth.slint). Modules shared across binaries carry `#![allow(dead_code)]`.

**`bt_backend` reads state over D-Bus.** `Snapshot::read()` makes one `busctl --json=short call org.bluez / ...ObjectManager GetManagedObjects` and parses every adapter, device (Alias, Icon, Paired/Trusted/Connected, RSSI, UUIDs) and `Battery1`, including per-component batteries on child objects. `with_audio_profiles()` adds card profiles from a single `pactl --format=json list cards`. Actions (connect, disconnect, trust, remove, power) are single `busctl call`/`set-property` invocations; success is the exit status, and the error text is the D-Bus error. `bluetoothctl` is used only for discovery (a long-lived `--timeout 60 scan on` child in `SCAN_CHILD`) and pairing (its agent answers the pairing request).

**`nm_backend` drives `nmcli`** in terse `-t` mode, parsed with `parse_nmcli_line`, which honours `\:` escapes. Other sources: `systemctl`/`warp-cli` for WARP, `/sys/class/net/hiddify-tun` or `pgrep` for Hiddify, `ping`, and `/proc/net/dev` for throughput. `list_networks_and_vpns` makes one `nmcli connection show` call plus one `device wifi list`.

**UI wiring.** `build.rs` compiles `ui/wifi.slint` and `ui/bluetooth.slint` into `OUT_DIR`, and each binary `include!`s its generated module. A new `.slint` file must be added to `build.rs` (both the `compile` and the `rerun-if-changed` lines). Each binary's GUI follows the same pattern:
- Slint callbacks spawn a `std::thread`, call the blocking backend, then hop back with `slint::invoke_from_event_loop` (the `set_status`/`apply_theme` helpers) using a `Weak` window handle. Never call a backend function on the UI thread.
- Background threads poll and repaint:
  - `auralink`: the network/VPN list every 10s, a Wi-Fi rescan on open, on refresh and every 60s, and the throughput graph every 200ms.
  - `auralink-bt`: the device list every 5s, plus the auto-connect loop.
  - Both: the pywal watcher every 2s.
- `ui/wifi.slint` declares `open_nmtui`, but no control invokes it.

**CLI dispatch.** `handle_commands(args) -> Option<Result<(), String>>` in each binary: `None` opens the GUI, and `Err` is printed and exits 1. Each binary's usage text is a single `USAGE` const.

## Bluetooth and Wi-Fi invariants (learned from bugs — keep them)

- **Don't use `bluetoothctl --timeout N` for one-shot commands.** It waits the full N seconds even after the command finishes (measured: `--timeout 4 show` takes 4.0s; without it, 6 ms). It also exits 0 on failure. Use `busctl`, or `output_within(cmd, limit)` when bluetoothctl is unavoidable. The one legitimate `--timeout` use is the discovery session's length.
- BlueZ reply texts change between versions. 5.88 says `Disconnection successful`, where older versions said `Successful disconnected`; matching the old text made every disconnect report failure. Prefer D-Bus exit status to text matching.
- **Discovery is a tracked child** (`start_scan`/`stop_scan`/`scan_running` kill and reap it). Don't `pkill` by pattern or drop the `Child`. BlueZ discovery is per-client, so `bluetoothctl scan off` from a new process can't stop ours. Never run discovery while an audio device is connected, because it makes A2DP stutter.
- **Never power the adapter on unattended.** Only the GUI's explicit refresh does that. The daemon used to undo Bluetooth-off and airplane mode.
- **Auto-connect policy lives in `autoconnect.rs`** (`AutoConnector::tick`), shared by `run_daemon()` and the GUI loop. The GUI loop stands down while `auralink-bt-daemon.service` is active and re-checks this on every pass.
  - It targets audio devices only (Icon `audio-*` or an A2DP-sink/HFP/HSP UUID), one at a time.
  - A device disconnected through `manual_disconnect` gets a marker under `$XDG_RUNTIME_DIR/auralink/bt-manual-disconnect/` and is skipped until `manual_connect`, pair or remove clears it. The marker is also cleared when the device is seen connected again, but only once it is more than 60s old.
  - User-initiated connect and disconnect must go through `manual_connect`/`manual_disconnect`, not the raw `bt_backend` calls.
- Don't hardcode `hci0`: device and adapter object paths come from the snapshot.
- `bluez-utils`, `pactl` and `notify-send` are optional at runtime. A missing tool must produce empty results or an error, never a panic. The release profile uses `panic = "abort"`, so a panic in any thread kills the app.
- **Poll Wi-Fi with `--rescan no`.** nmcli's default `--rescan auto` blocks on a fresh scan whenever results are over 30s old. It made `auralink waybar-stream` take 5.5s and kept the radio scanning.
- `nmcli device disconnect` takes an interface name (e.g. `wlan0`), not a type such as `wifi`.
- Wi-Fi passwords are still passed to `nmcli` as argv in `connect_to_wifi` and `update_network_config`, where other local users can see them in `ps`. Fixing this needs testing against a real network join.

## Packaging and release

- The version lives in **both** `Cargo.toml` and `PKGBUILD` (`pkgver`); bump them together and refresh `Cargo.lock`.
- A file installed onto the system has to be listed in four places: `PKGBUILD` `package()`, `[package.metadata.deb].assets`, `[package.metadata.generate-rpm].assets` in `Cargo.toml`, and `install.sh`.
- `auralink-bt-daemon.service` is a **user** unit with `ExecStart=/usr/bin/auralink-bt daemon` for the packages. `install.sh` rewrites it to `%h/.local/bin` and stops the running daemon before replacing the binary, because overwriting a running executable fails with "Text file busy".
- Packages are built from `git archive HEAD` (prefix `auralink-build/`), so uncommitted changes are not included.
- Pushing a `v*` tag runs `.github/workflows/release.yml`, which builds deb (bookworm), rpm (fedora), and Arch (`archlinux:base-devel`, as a non-root user) in containers and publishes a GitHub release.
- The repo lives on a filesystem that reports every file as mode 777. `install.sh` `chmod 644`s the unit because systemd rejects a world-writable one.

## Untracked clutter in the working tree

`AppDir/`, `squashfs-root/`, `pkg/`, `auralink/` (a stray bare git repo), `output/`, `dist/`, `build/`, `linuxdeploy`, `*.tar.gz`, and `test_slint*`/`test.slint` are gitignored build or scratch artifacts, not source. Don't edit them or search them for code. `docs/superpowers/` holds the design spec and implementation plan for the `status`/`fullstatus` commands.
