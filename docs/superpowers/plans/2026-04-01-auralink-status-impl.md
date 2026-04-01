# Auralink CLI Status Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement `status`, `fullstatus`, and `--help` commands for `auralink` and `auralink-bt` with structured JSON output.

**Architecture:** Manual argument matching in `main()` before GUI initialization. Use `serde_json::json!` for output.

**Tech Stack:** Rust, serde_json, nmcli (via nm_backend), bluetoothctl (via bt_backend).

---

### Task 1: Implement `auralink-bt` status commands

**Files:**
- Modify: `src/bt_main.rs`

- [ ] **Step 1: Refactor `main` to handle `status`, `fullstatus`, and `--help`**

```rust
fn main() -> Result<(), slint::PlatformError> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        match args[1].as_str() {
            "status" => {
                let powered = bt_backend::is_powered();
                let connected_devices = bt_backend::list_connected_devices();
                let output = serde_json::json!({
                    "powered": powered,
                    "connected_devices": connected_devices
                });
                println!("{}", output);
                return Ok(());
            }
            "fullstatus" => {
                let powered = bt_backend::is_powered();
                let connected_devices = bt_backend::list_connected_devices();
                let all_devices = bt_backend::list_devices();
                let output = serde_json::json!({
                    "status": {
                        "powered": powered,
                        "connected_devices": connected_devices
                    },
                    "all_devices": all_devices
                });
                println!("{}", output);
                return Ok(());
            }
            "--help" | "-h" | "help" => {
                println!("Usage: auralink-bt [COMMAND]\n\nCommands:\n  status      Get current connection status (JSON)\n  fullstatus  Get detailed status including available devices (JSON)\n  --help      Show this help message");
                return Ok(());
            }
            _ => {
                // If invalid command, show help and exit or just start GUI? 
                // Let's show help for now.
                println!("Unknown command: {}\n", args[1]);
                println!("Usage: auralink-bt [COMMAND]\n\nCommands:\n  status      Get current connection status (JSON)\n  fullstatus  Get detailed status including available devices (JSON)\n  --help      Show this help message");
                return Ok(());
            }
        }
    }
    // Existing GUI code follows...
```

- [ ] **Step 2: Commit changes**

```bash
git add src/bt_main.rs
git commit -m "feat(bt): implement status and fullstatus commands"
```

### Task 2: Implement `auralink` status commands

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add `status`, `fullstatus`, and `--help` to `main`**

```rust
#[tokio::main]
async fn main() -> Result<(), slint::PlatformError> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        match args[1].as_str() {
            "status" => {
                let networks = nm_backend::list_networks();
                let active_network = networks.iter().find(|n| n.connected);
                let active_vpns = nm_backend::list_vpns().into_iter().filter(|v| v.active).collect::<Vec<_>>();
                let iface = nm_backend::get_active_interface().unwrap_or_default();
                let stats = nm_backend::get_interface_stats(&iface).map(|(rx, tx)| serde_json::json!({ "rx": rx, "tx": tx }));
                
                let output = serde_json::json!({
                    "active_network": active_network,
                    "active_vpns": active_vpns,
                    "interface_stats": stats,
                    "is_scanning": false
                });
                println!("{}", output);
                return Ok(());
            }
            "fullstatus" => {
                let networks = nm_backend::list_networks();
                let active_network = networks.iter().find(|n| n.connected);
                let active_vpns = nm_backend::list_vpns().into_iter().filter(|v| v.active).collect::<Vec<_>>();
                let iface = nm_backend::get_active_interface().unwrap_or_default();
                let stats = nm_backend::get_interface_stats(&iface).map(|(rx, tx)| serde_json::json!({ "rx": rx, "tx": tx }));
                
                let output = serde_json::json!({
                    "status": {
                        "active_network": active_network,
                        "active_vpns": active_vpns,
                        "interface_stats": stats,
                        "is_scanning": false
                    },
                    "available_networks": networks
                });
                println!("{}", output);
                return Ok(());
            }
            "--help" | "-h" | "help" => {
                println!("Usage: auralink [COMMAND]\n\nCommands:\n  status      Get current connection status (JSON)\n  fullstatus  Get detailed status including available networks (JSON)\n  --help      Show this help message");
                return Ok(());
            }
            _ => {
                println!("Unknown command: {}\n", args[1]);
                println!("Usage: auralink [COMMAND]\n\nCommands:\n  status      Get current connection status (JSON)\n  fullstatus  Get detailed status including available networks (JSON)\n  --help      Show this help message");
                return Ok(());
            }
        }
    }
    // Existing GUI code follows...
```

- [ ] **Step 2: Commit changes**

```bash
git add src/main.rs
git commit -m "feat(net): implement status and fullstatus commands"
```

### Task 3: Verification

- [ ] **Step 1: Compile and verify `auralink-bt`**

Run: `cargo run --bin auralink-bt -- status`
Run: `cargo run --bin auralink-bt -- fullstatus`
Run: `cargo run --bin auralink-bt -- --help`

- [ ] **Step 2: Compile and verify `auralink`**

Run: `cargo run --bin auralink -- status`
Run: `cargo run --bin auralink -- fullstatus`
Run: `cargo run --bin auralink -- --help`
