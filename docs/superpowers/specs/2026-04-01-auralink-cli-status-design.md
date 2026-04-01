# Auralink CLI Status Design

This document describes the implementation of `status` and `fullstatus` commands for `auralink` and `auralink-bt` to provide structured JSON output for status updates.

## Goals
- Add `status` and `fullstatus` commands to both `auralink` and `auralink-bt`.
- Provide machine-readable JSON output for easy integration with status bars and scripts.
- Add a `--help` flag to both binaries.

## 1. `auralink` (Network/VPN)

### `auralink status`
Outputs JSON for the currently active connection and basic interface stats.

**Data structure:**
- `active_network`: SSID, signal, security, connected status.
- `active_vpns`: List of active VPN names and types.
- `interface_stats`: Current RX/TX bytes from `/proc/net/dev`.
- `is_scanning`: Boolean (derived from recent nmcli activity if possible, or just false by default).

### `auralink fullstatus`
Includes `status` output plus the full list of available Wi-Fi networks.

**Data structure:**
- `status`: Same as `auralink status`.
- `available_networks`: List of all scanned networks (SSID, BSSID, Signal, Security, Saved, Connected).

## 2. `auralink-bt` (Bluetooth)

### `auralink-bt status`
Outputs JSON for the current Bluetooth power state and connected devices.

**Data structure:**
- `powered`: Boolean.
- `connected_devices`: List of devices with Name, Address, Battery, and Connected status.

### `auralink-bt fullstatus`
Includes `status` output plus all paired/available devices and their supported audio profiles.

**Data structure:**
- `status`: Same as `auralink-bt status`.
- `all_devices`: List of all devices known to bluetoothctl, including:
    - Name, Address, Connected, Paired, Trusted, RSSI, Battery.
    - `audio_profiles`: List of profiles (Name, Description, Active, Available) for connected devices.

## 3. `--help` Flag
Both binaries will catch `--help` (or no arguments/invalid arguments) and print a simple usage message:

```text
Usage: auralink [COMMAND]
Commands:
  status      Get current connection status (JSON)
  fullstatus  Get detailed status including available networks (JSON)
  --help      Show this help message
```

## Implementation Details
- **Argument Parsing:** Manual matching of `std::env::args()` in `main.rs` and `bt_main.rs`.
- **JSON Serialization:** Use `serde_json::json!` for easy composition of the output.
- **Backend Integration:** Leverage existing functions in `nm_backend.rs` and `bt_backend.rs`.

## Testing Strategy
- Manual verification: Run `auralink status` and `auralink-bt status` to check JSON validity and content.
- Verify `--help` output.
- Verify `fullstatus` includes expected additional fields.
