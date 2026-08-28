mod bt_backend;

use slint::{VecModel, Color, ModelRc, ComponentHandle};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

mod ui {
    include!(concat!(env!("OUT_DIR"), "/bluetooth.rs"));
}
use ui::*;

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct AppConfig {
    pywal_enabled: bool,
}

impl AppConfig {
    fn load() -> Self {
        let home = std::env::var("HOME").unwrap_or_default();
        let path = format!("{}/.config/auralink/bt_config.json", home);
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<AppConfig>(&s).ok())
            .unwrap_or(AppConfig { 
                pywal_enabled: false,
            })
    }

    fn save(&self) {
        let home = std::env::var("HOME").unwrap_or_default();
        let dir = format!("{}/.config/auralink", home);
        let path = format!("{}/bt_config.json", dir);
        let _ = std::fs::create_dir_all(dir);
        if let Ok(s) = serde_json::to_string(self) {
            let _ = std::fs::write(path, s);
        }
    }
}

fn send_notification(summary: &str, body: &str, icon: Option<&str>) {
    let mut cmd = std::process::Command::new("notify-send");
    cmd.args([
        "-a", "AuraLink BT",
        "-t", "3500",
        "-u", "normal",
        summary,
        body,
    ]);
    if let Some(ic) = icon {
        cmd.args(["-i", ic]);
    }
    let _ = cmd.spawn();
}

#[derive(serde::Deserialize, Debug)]
struct PywalColors {
    colors: std::collections::HashMap<String, String>,
    special: std::collections::HashMap<String, String>,
}

fn parse_hex(hex: &str) -> Option<Color> {
    let hex = hex.trim_start_matches('#');
    if hex.len() == 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some(Color::from_rgb_u8(r, g, b))
    } else if hex.len() == 8 {
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
        Some(Color::from_argb_u8(a, r, g, b))
    } else {
        None
    }
}

fn apply_pywal_theme(handle: slint::Weak<AppWindow>) {
    let home = std::env::var("HOME").unwrap_or_default();
    let path = format!("{}/.cache/wal/colors.json", home);
    
    if let Ok(content) = std::fs::read_to_string(path)
        && let Ok(wal) = serde_json::from_str::<PywalColors>(&content) {
            let mut bg = parse_hex(wal.special.get("background").unwrap_or(&"#09090b".to_string())).unwrap_or(Color::from_rgb_u8(9, 9, 11));
            bg = Color::from_argb_u8(136, bg.red(), bg.green(), bg.blue());
            
            let fg = parse_hex(wal.special.get("foreground").unwrap_or(&"#f8fafc".to_string())).unwrap_or(Color::from_rgb_u8(248, 250, 252));
            let accent = parse_hex(wal.colors.get("color1").unwrap_or(&"#00f0ff".to_string())).unwrap_or(Color::from_rgb_u8(0, 240, 255));
            
            let card_bg = Color::from_argb_u8(255, 
                (bg.red() as i16 + 15).clamp(0, 255) as u8,
                (bg.green() as i16 + 15).clamp(0, 255) as u8,
                (bg.blue() as i16 + 15).clamp(0, 255) as u8
            );

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = handle.upgrade() {
                    let palette = ui.global::<Palette>();
                    palette.set_background(bg);
                    palette.set_foreground(fg);
                    palette.set_accent(accent);
                    palette.set_card_bg(card_bg);
                    palette.set_secondary_fg(Color::from_argb_u8(180, fg.red(), fg.green(), fg.blue()));
                    palette.set_separator(Color::from_argb_u8(60, fg.red(), fg.green(), fg.blue()));
                }
            });
        }
}

// Intermediate struct that IS Send
struct InternalBluetoothDevice {
    name: String,
    address: String,
    connected: bool,
    paired: bool,
    trusted: bool,
    rssi: i32,
    battery: Option<i32>,
    batteries: Vec<bt_backend::Battery>,
    audio_profiles: Vec<bt_backend::AudioProfile>,
}

fn run_daemon() {
    // Poll cadence while nothing is connected. Each tick costs a couple of
    // short-lived helper processes, so it stays modest.
    const IDLE_POLL: Duration = Duration::from_secs(8);
    // Cadence once something IS connected; nothing to do but notice a drop.
    const CONNECTED_POLL: Duration = Duration::from_secs(15);
    // Per-device backoff so a device that is simply switched off is not
    // hammered every tick.
    const RETRY_COOLDOWN: Duration = Duration::from_secs(30);

    eprintln!("=== AuraLink Bluetooth Auto-Connect Daemon Started ===");
    let _ = bt_backend::ensure_switch_on_connect_module();

    let mut last_attempt: std::collections::HashMap<String, Instant> = std::collections::HashMap::new();

    loop {
        if let Some(connected) = bt_backend::get_connected_address() {
            // Discovery competes with A2DP for the radio and audibly stutters
            // playback, so it must not run while a headset is streaming.
            if bt_backend::scan_running() {
                eprintln!("Daemon: {} connected; stopping discovery.", connected);
                bt_backend::stop_scan();
            }
            last_attempt.clear();
            std::thread::sleep(CONNECTED_POLL);
            continue;
        }

        let trusted: Vec<String> = bt_backend::list_trusted();
        if trusted.is_empty() {
            // Nothing to reconnect to; do not spin the radio.
            if bt_backend::scan_running() {
                bt_backend::stop_scan();
            }
            std::thread::sleep(IDLE_POLL);
            continue;
        }

        // Auto-discovery. This was missing entirely: the daemon only ever
        // called `connect`, and a trusted device that has been out of range
        // (earbuds in their case) is not reliably pageable until the adapter
        // has seen it advertise again. Keeping a discovery session alive while
        // disconnected is what makes reconnect-on-power-up actually work.
        // bluetoothctl's own session expires after SCAN_SESSION_SECS, so
        // scan_running() going false is the cue to start the next one.
        if !bt_backend::scan_running() {
            bt_backend::start_scan();
        }

        let now = Instant::now();
        for address in trusted {
            if let Some(last_time) = last_attempt.get(&address)
                && now.duration_since(*last_time) < RETRY_COOLDOWN {
                    continue;
                }

            // Something may have connected while we worked through the list.
            if bt_backend::get_connected_address().is_some() {
                break;
            }

            eprintln!("Daemon: attempting auto-connect to trusted device {}", address);
            last_attempt.insert(address.clone(), now);
            bt_backend::connect_trusted_device(&address);

            // Ask BlueZ what actually happened rather than trusting the
            // return value: bluetoothctl reports failures in its text and
            // still exits 0, so the old code logged "Successfully
            // auto-connected" for every failed attempt.
            if bt_backend::get_connected_address().as_deref() == Some(address.as_str()) {
                eprintln!("Daemon: connected to {}", address);
                bt_backend::stop_scan();
                // Give BlueZ a moment to register the audio card with
                // PipeWire before selecting a profile on it.
                std::thread::sleep(Duration::from_secs(2));
                if bt_backend::force_a2dp_profile(&address) {
                    eprintln!("Daemon: selected A2DP profile for {}", address);
                }
                break;
            }

            eprintln!("Daemon: {} did not connect (out of range or powered off)", address);
        }

        std::thread::sleep(IDLE_POLL);
    }
}

fn handle_commands(args: &[String]) -> Option<Result<(), Box<dyn std::error::Error>>> {
    if args.len() <= 1 {
        return None;
    }

    match args[1].as_str() {
        "popup" | "gui" | "open" => {
            None
        }
        "quickshell" | "qs" => {
            let home = std::env::var("HOME").unwrap_or_default();
            let path = format!("{}/.config/quickshell/auralink", home);
            let _ = std::process::Command::new("quickshell")
                .args(["-p", &path])
                .spawn();
            Some(Ok(()))
        }
        "waybar-stream" => {
            let powered = bt_backend::is_powered();
            if !powered {
                println!("{}", serde_json::json!({
                    "text": "󰂲",
                    "tooltip": "Bluetooth: Off",
                    "class": "off"
                }));
            } else {
                let connected_devices = bt_backend::list_connected_devices();
                if let Some(dev) = connected_devices.first() {
                    let battery_str = dev.battery.map(|b| format!(" ({}%)", b)).unwrap_or_default();
                    println!("{}", serde_json::json!({
                        "text": "󰂱",
                        "tooltip": format!("Bluetooth: Connected to {}{}", dev.name, battery_str),
                        "class": "connected"
                    }));
                } else {
                    println!("{}", serde_json::json!({
                        "text": "󰂯",
                        "tooltip": "Bluetooth: Disconnected",
                        "class": "disconnected"
                    }));
                }
            }
            Some(Ok(()))
        }
        "connect" => {
            if args.len() < 3 {
                eprintln!("Usage: auralink-bt connect <MAC_ADDRESS>");
                return Some(Ok(()));
            }
            let mac = &args[2];
            println!("Connecting to {}...", mac);
            let success = bt_backend::connect(mac);
            if success {
                println!("Successfully connected to {}", mac);
            } else {
                eprintln!("Failed to connect to {}", mac);
            }
            Some(Ok(()))
        }
        "disconnect" => {
            if args.len() < 3 {
                eprintln!("Usage: auralink-bt disconnect <MAC_ADDRESS>");
                return Some(Ok(()));
            }
            let mac = &args[2];
            println!("Disconnecting {}...", mac);
            let success = bt_backend::disconnect(mac);
            if success {
                println!("Disconnected {}", mac);
            } else {
                eprintln!("Failed to disconnect {}", mac);
            }
            Some(Ok(()))
        }
        "pair" => {
            if args.len() < 3 {
                eprintln!("Usage: auralink-bt pair <MAC_ADDRESS>");
                return Some(Ok(()));
            }
            let mac = &args[2];
            println!("Pairing with {}...", mac);
            let success = bt_backend::pair(mac);
            if success {
                println!("Paired with {}", mac);
            } else {
                eprintln!("Failed to pair with {}", mac);
            }
            Some(Ok(()))
        }
        "trust" => {
            if args.len() < 3 {
                eprintln!("Usage: auralink-bt trust <MAC_ADDRESS>");
                return Some(Ok(()));
            }
            let mac = &args[2];
            println!("Trusting {}...", mac);
            let success = bt_backend::trust(mac, true);
            if success {
                println!("Trusted {}", mac);
            } else {
                eprintln!("Failed to trust {}", mac);
            }
            Some(Ok(()))
        }
        "remove" | "unpair" => {
            if args.len() < 3 {
                eprintln!("Usage: auralink-bt remove <MAC_ADDRESS>");
                return Some(Ok(()));
            }
            let mac = &args[2];
            println!("Removing {}...", mac);
            let success = bt_backend::remove(mac);
            if success {
                println!("Removed {}", mac);
            } else {
                eprintln!("Failed to remove {}", mac);
            }
            Some(Ok(()))
        }
        "daemon" | "--daemon" | "-d" => {
            run_daemon();
            Some(Ok(()))
        }
        "status" => {
            let powered = bt_backend::is_powered();
            let connected_devices = bt_backend::list_connected_devices();
            println!("{}", serde_json::json!({
                "powered": powered,
                "connected_devices": connected_devices
            }));
            Some(Ok(()))
        }
        "fullstatus" => {
            let powered = bt_backend::is_powered();
            let connected_devices = bt_backend::list_connected_devices();
            let all_devices = bt_backend::list_devices();
            println!("{}", serde_json::json!({
                "status": {
                    "powered": powered,
                    "connected_devices": connected_devices
                },
                "all_devices": all_devices
            }));
            Some(Ok(()))
        }
        "--help" | "-h" | "help" => {
            println!("Usage: auralink-bt [COMMAND]");
            println!();
            println!("Commands:");
            println!("  popup            Open the Bluetooth GUI interface");
            println!("  connect <MAC>    Connect to a Bluetooth device");
            println!("  disconnect <MAC> Disconnect a Bluetooth device");
            println!("  pair <MAC>       Pair with a Bluetooth device");
            println!("  trust <MAC>      Trust a Bluetooth device");
            println!("  remove <MAC>     Remove/unpair a Bluetooth device");
            println!("  toggle           Toggle Bluetooth adapter power");
            println!("  daemon           Run auto-connect background daemon (headless, no GUI)");
            println!("  status           Get current connection status (JSON)");
            println!("  fullstatus       Get detailed status including available devices (JSON)");
            println!("  waybar-stream    Stream Waybar module output (JSON)");
            println!("  --help           Show this help message");
            Some(Ok(()))
        }
        "toggle" => {
            let powered = bt_backend::is_powered();
            bt_backend::set_power(!powered);
            println!("Bluetooth power toggled {}", if !powered { "ON" } else { "OFF" });
            Some(Ok(()))
        }
        cmd => {
            eprintln!("Error: Unknown command '{}'", cmd);
            println!("Usage: auralink-bt [COMMAND]");
            println!();
            println!("Commands:");
            println!("  popup            Open the Bluetooth GUI interface");
            println!("  connect <MAC>    Connect to a Bluetooth device");
            println!("  disconnect <MAC> Disconnect a Bluetooth device");
            println!("  pair <MAC>       Pair with a Bluetooth device");
            println!("  trust <MAC>      Trust a Bluetooth device");
            println!("  remove <MAC>     Remove/unpair a Bluetooth device");
            println!("  toggle           Toggle Bluetooth adapter power");
            println!("  daemon           Run auto-connect background daemon (headless, no GUI)");
            println!("  status           Get current connection status (JSON)");
            println!("  fullstatus       Get detailed status including available devices (JSON)");
            println!("  waybar-stream    Stream Waybar module output (JSON)");
            println!("  --help           Show this help message");
            Some(Ok(()))
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if let Some(res) = handle_commands(&args) {
        return res;
    }

    let main_window = AppWindow::new().map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    let config = Arc::new(Mutex::new(AppConfig::load()));
    
    if let Ok(cfg) = config.lock() {
        main_window.set_pywal_enabled(cfg.pywal_enabled);
        if cfg.pywal_enabled {
            apply_pywal_theme(main_window.as_weak());
        }
    }

    let window_weak = main_window.as_weak();
    main_window.on_toggle_settings(move || {
        if let Some(ui) = window_weak.upgrade() {
            ui.set_show_settings(!ui.get_show_settings());
        }
    });

    let config_clone = config.clone();
    let window_weak = main_window.as_weak();
    main_window.on_toggle_pywal(move |enabled| {
        if let Ok(mut cfg) = config_clone.lock() {
            cfg.pywal_enabled = enabled;
            cfg.save();
            if enabled {
                apply_pywal_theme(window_weak.clone());
            } else {
                let handle = window_weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = handle.upgrade() {
                        let p = ui.global::<Palette>();
                        p.set_background(Color::from_argb_u8(136, 9, 9, 11));
                        p.set_card_bg(Color::from_rgb_u8(24, 24, 27));
                        p.set_accent(Color::from_rgb_u8(0, 240, 255));
                        p.set_foreground(Color::from_rgb_u8(248, 250, 252));
                        p.set_secondary_fg(Color::from_rgb_u8(161, 161, 170));
                        p.set_separator(Color::from_rgb_u8(39, 39, 42));
                    }
                });
            }
        }
    });

    main_window.on_toggle_bluetooth(|enabled| {
        bt_backend::set_power(enabled);
    });

    let window_weak = main_window.as_weak();
    main_window.on_connect(move |address| {
        let address = address.to_string();
        let ww = window_weak.clone();
        std::thread::spawn(move || {
            let success = bt_backend::connect(&address);
            let addr_msg = address.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ww.upgrade() {
                    ui.set_status_msg(if success { format!("Connected to {}", addr_msg) } else { "Failed!".to_string() }.into());
                }
            });
            if success {
                send_notification("Bluetooth Connected", &format!("Connected to {}", address), Some("bluetooth-active"));
            } else {
                send_notification("Bluetooth Connection Error", &format!("Failed to connect to {}", address), Some("dialog-error"));
            }
        });
    });

    let window_weak = main_window.as_weak();
    main_window.on_disconnect(move |address| {
        let address = address.to_string();
        let ww = window_weak.clone();
        std::thread::spawn(move || {
            let success = bt_backend::disconnect(&address);
            let addr_msg = address.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ww.upgrade() {
                    ui.set_status_msg(if success { format!("Disconnected {}", addr_msg) } else { "Failed!".to_string() }.into());
                }
            });
            if success {
                send_notification("Bluetooth Disconnected", &format!("Disconnected {}", address), Some("bluetooth-disabled"));
            } else {
                send_notification("Bluetooth Error", &format!("Failed to disconnect {}", address), Some("dialog-error"));
            }
        });
    });

    let window_weak = main_window.as_weak();
    main_window.on_toggle_trust(move |address, trust| {
        let address = address.to_string();
        let ww = window_weak.clone();
        std::thread::spawn(move || {
            let success = bt_backend::trust(&address, trust);
            if !success {
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ww.upgrade() {
                        ui.set_status_msg(format!("Trust failed for {}", address).into());
                    }
                });
            } else {
                eprintln!("Trust successful for {}, triggering scan refresh...", address);
                std::thread::sleep(Duration::from_millis(500));
                let _ = bt_backend::start_scan();
                let addr = address.clone();
                let t = trust;
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ww.upgrade() {
                        ui.set_status_msg(format!("{}ed {}", if t { "Trust" } else { "Untrust" }, addr).into());
                    }
                });
            }
        });
    });

    let window_weak = main_window.as_weak();
    main_window.on_toggle_pair(move |address, pair| {
        let address = address.to_string();
        let ww = window_weak.clone();
        std::thread::spawn(move || {
            let success = if pair {
                bt_backend::pair(&address)
            } else {
                bt_backend::remove(&address)
            };
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ww.upgrade() {
                    ui.set_status_msg(if success {
                        if pair { format!("Paired {}", address).into() } else { format!("Removed {}", address).into() }
                    } else {
                        format!("Failed to {} for {}", if pair { "pair" } else { "unpair" }, address).into()
                    });
                }
            });
        });
    });

    let window_weak = main_window.as_weak();
    main_window.on_remove_device(move |address| {
        let address = address.to_string();
        let ww = window_weak.clone();
        std::thread::spawn(move || {
            let success = bt_backend::remove(&address);
            if !success {
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ww.upgrade() {
                        ui.set_status_msg(format!("Remove failed for {}", address).into());
                    }
                });
            }
        });
    });

    let window_weak = main_window.as_weak();
    main_window.on_select_audio_profile(move |address, profile| {
        let address = address.to_string();
        let profile = profile.to_string();
        let ww = window_weak.clone();
        std::thread::spawn(move || {
            let success = bt_backend::set_audio_profile(&address, &profile);
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ww.upgrade() {
                    ui.set_status_msg(if success { format!("Profile set: {}", profile) } else { "Profile set failed".to_string() }.into());
                }
            });
        });
    });

    let window_weak = main_window.as_weak();
    main_window.on_expand_device(move |address| {
        if let Some(ui) = window_weak.upgrade() {
            ui.set_expanded_address(address);
        }
    });

    let window_weak = main_window.as_weak();
    main_window.on_refresh(move || {
        if let Some(ui) = window_weak.upgrade() {
            ui.set_is_scanning(true);
            ui.set_status_msg("SCANNING...".into());
            
            let handle = window_weak.clone();
            std::thread::spawn(move || {
                let _ = bt_backend::start_scan();
                std::thread::sleep(Duration::from_secs(15));
                let _ = bt_backend::stop_scan();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = handle.upgrade() {
                        ui.set_is_scanning(false);
                        ui.set_status_msg("READY".into());
                    }
                });
            });
        }
    });

    // Initial scan on startup
    let window_weak = main_window.as_weak();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = window_weak.upgrade() {
                ui.invoke_refresh();
            }
        });
    });

    // Unified intelligent Auto-Connect & Connection Monitor Loop
    let window_weak = main_window.as_weak();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(2));

        eprintln!("=== Auto-connect & Connection Monitor loop starting ===");
        let _ = bt_backend::ensure_switch_on_connect_module();

        let mut last_attempt: std::collections::HashMap<String, Instant> = std::collections::HashMap::new();

        loop {
            let connected_address = bt_backend::get_connected_address();

            if connected_address.is_some() {
                // Device is connected! Ensure background scanning is stopped to preserve A2DP stability.
                bt_backend::stop_scan();
            } else {
                // No device is connected. Attempt to connect trusted devices with a cooldown.
                let trusted: Vec<String> = bt_backend::list_trusted();
                let now = Instant::now();

                for address in trusted {
                    // Check if we tried connecting to this address recently (cooldown of 30s)
                    if let Some(last_time) = last_attempt.get(&address) {
                        if now.duration_since(*last_time) < Duration::from_secs(30) {
                            continue;
                        }
                    }

                    // Check again if a device connected in the meantime
                    if bt_backend::get_connected_address().is_some() {
                        break;
                    }

                    eprintln!("Attempting auto-connect for trusted device: {}", address);
                    last_attempt.insert(address.clone(), now);

                    let success = bt_backend::connect_trusted_device(&address);
                    let addr_clone = address.clone();
                    let ww = window_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ww.upgrade() {
                            ui.set_status_msg(if success {
                                format!("Auto-connected {}", addr_clone).into()
                            } else {
                                format!("Auto-connect failed {}", addr_clone).into()
                            });
                        }
                    });

                    if success {
                        bt_backend::stop_scan();
                        std::thread::sleep(Duration::from_secs(2));
                        let _ = bt_backend::force_a2dp_profile(&address);
                        break;
                    }
                }
            }

            std::thread::sleep(Duration::from_secs(10));
        }
    });

    let window_weak = main_window.as_weak();
    std::thread::spawn(move || {
        loop {
            let devices = bt_backend::list_devices();
            let internal_devices: Vec<InternalBluetoothDevice> = devices.into_iter().map(|d| {
                InternalBluetoothDevice {
                    name: d.name,
                    address: d.address,
                    connected: d.connected,
                    paired: d.paired,
                    trusted: d.trusted,
                    rssi: d.rssi,
                    battery: d.battery,
                    batteries: d.batteries,
                    audio_profiles: d.audio_profiles,
                }
            }).collect();

            let ww = window_weak.clone();
            let powered = bt_backend::is_powered();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ww.upgrade() {
                    let slint_devices: Vec<BluetoothDevice> = internal_devices.into_iter().map(|d| {
                        let (icon, icon_color) = if d.connected { ("󰂱", Color::from_rgb_u8(0, 240, 255)) }
                                                 else if d.paired { ("󰂲", Color::from_rgb_u8(245, 194, 17)) }
                                                 else { ("󰂯", Color::from_rgb_u8(161, 161, 170)) };
                        
                        let slint_profiles: Vec<AudioProfile> = d.audio_profiles.into_iter().map(|p| {
                            AudioProfile {
                                name: p.name.into(),
                                description: p.description.into(),
                                active: p.active,
                                available: p.available,
                            }
                        }).collect();

                        let slint_batteries: Vec<BatteryInfo> = d.batteries.into_iter().map(|b| {
                            BatteryInfo {
                                label: b.label.into(),
                                percentage: b.percentage,
                            }
                        }).collect();

                        BluetoothDevice {
                            name: d.name.into(),
                            address: d.address.into(),
                            connected: d.connected,
                            paired: d.paired,
                            trusted: d.trusted,
                            rssi: d.rssi,
                            battery: d.battery.unwrap_or(0),
                            batteries: ModelRc::new(VecModel::from(slint_batteries)),
                            icon: icon.into(),
                            icon_color,
                            audio_profiles: ModelRc::new(VecModel::from(slint_profiles)),
                        }
                    }).collect();

                    ui.set_devices(ModelRc::new(VecModel::from(slint_devices)));
                    ui.set_bluetooth_enabled(powered);
                }
            });

            std::thread::sleep(Duration::from_secs(5));
        }
    });

    main_window.run().map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_commands_none() {
        let args = vec!["auralink-bt".to_string()];
        assert!(handle_commands(&args).is_none());
    }

    #[test]
    fn test_handle_commands_help() {
        let args = vec!["auralink-bt".to_string(), "--help".to_string()];
        assert!(handle_commands(&args).is_some());
    }

    #[test]
    fn test_handle_commands_status() {
        let args = vec!["auralink-bt".to_string(), "status".to_string()];
        assert!(handle_commands(&args).is_some());
    }

    #[test]
    fn test_handle_commands_fullstatus() {
        let args = vec!["auralink-bt".to_string(), "fullstatus".to_string()];
        assert!(handle_commands(&args).is_some());
    }

    #[test]
    fn test_handle_commands_unknown() {
        let args = vec!["auralink-bt".to_string(), "unknown".to_string()];
        assert!(handle_commands(&args).is_some());
    }
}
