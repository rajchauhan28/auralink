mod autoconnect;
mod bt_backend;
mod common;

use common::{AppConfig, Theme};
use slint::{Color, ComponentHandle, ModelRc, VecModel};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

mod ui {
    include!(concat!(env!("OUT_DIR"), "/bluetooth.rs"));
}
use ui::*;

const CONFIG_FILE: &str = "bt_config.json";

fn notify(summary: &str, body: &str, icon: Option<&str>) {
    common::send_notification("AuraLink BT", summary, body, icon);
}

fn set_palette(ui: &AppWindow, t: &Theme) {
    let p = ui.global::<Palette>();
    p.set_background(t.background);
    p.set_card_bg(t.card_bg);
    p.set_accent(t.accent);
    p.set_foreground(t.foreground);
    p.set_secondary_fg(t.secondary_fg);
    p.set_separator(t.separator);
}

/// Hop to the UI thread and apply a theme.
fn apply_theme(handle: slint::Weak<AppWindow>, theme: Theme) {
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = handle.upgrade() {
            set_palette(&ui, &theme);
        }
    });
}

/// Hop to the UI thread and set the status line.
fn set_status(handle: &slint::Weak<AppWindow>, msg: String) {
    let handle = handle.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = handle.upgrade() {
            ui.set_status_msg(msg.into());
        }
    });
}

fn to_slint(d: bt_backend::BluetoothDevice) -> BluetoothDevice {
    let (icon, icon_color) = if d.connected {
        ("󰂱", Color::from_rgb_u8(0, 240, 255))
    } else if d.paired {
        ("󰂲", Color::from_rgb_u8(245, 194, 17))
    } else {
        ("󰂯", Color::from_rgb_u8(161, 161, 170))
    };
    let profiles: Vec<AudioProfile> = d
        .audio_profiles
        .into_iter()
        .map(|p| AudioProfile {
            name: p.name.into(),
            description: p.description.into(),
            active: p.active,
            available: p.available,
        })
        .collect();
    let batteries: Vec<BatteryInfo> = d
        .batteries
        .into_iter()
        .map(|b| BatteryInfo {
            label: b.label.into(),
            percentage: b.percentage,
        })
        .collect();
    BluetoothDevice {
        name: d.name.into(),
        address: d.address.into(),
        connected: d.connected,
        paired: d.paired,
        trusted: d.trusted,
        rssi: d.rssi,
        battery: d.battery.unwrap_or(0),
        batteries: ModelRc::new(VecModel::from(batteries)),
        icon: icon.into(),
        icon_color,
        audio_profiles: ModelRc::new(VecModel::from(profiles)),
    }
}

const USAGE: &str = "\
Usage: auralink-bt [COMMAND]

Commands:
  popup            Open the Bluetooth GUI interface
  connect <MAC>    Connect to a Bluetooth device
  disconnect <MAC> Disconnect a device (auto-connect leaves it alone until you reconnect it)
  pair <MAC>       Pair with a Bluetooth device
  trust <MAC>      Trust a Bluetooth device
  remove <MAC>     Remove/unpair a Bluetooth device
  toggle           Toggle Bluetooth adapter power
  daemon           Run auto-connect background daemon (headless, no GUI)
  status           Get current connection status (JSON)
  fullstatus       Get detailed status including available devices (JSON)
  waybar-stream    Print a Waybar module status line (JSON)
  --help           Show this help message";

/// Run a device action from the CLI, printing progress and the outcome.
fn run_action(
    args: &[String],
    verb: &str,
    doing: &str,
    done: &str,
    action: fn(&str) -> Result<(), String>,
) -> Result<(), String> {
    let mac = args
        .get(2)
        .ok_or_else(|| format!("Usage: auralink-bt {} <MAC_ADDRESS>", verb))?;
    println!("{} {}...", doing, mac);
    action(mac).map_err(|e| format!("Failed to {} {}: {}", verb, mac, e))?;
    println!("{} {}", done, mac);
    Ok(())
}

/// Handle a CLI subcommand. None means "open the GUI".
fn handle_commands(args: &[String]) -> Option<Result<(), String>> {
    let cmd = args.get(1)?;
    let result = match cmd.as_str() {
        "popup" | "gui" | "open" => return None,
        "quickshell" | "qs" => {
            let path = format!(
                "{}/.config/quickshell/auralink",
                std::env::var("HOME").unwrap_or_default()
            );
            std::process::Command::new("quickshell")
                .args(["-p", &path])
                .spawn()
                .map(|_| ())
                .map_err(|e| format!("cannot start quickshell: {}", e))
        }
        "waybar-stream" => {
            // One D-Bus read; Waybar runs this every few seconds.
            let snap = bt_backend::Snapshot::read();
            let out = if !snap.powered() {
                serde_json::json!({"text": "󰂲", "tooltip": "Bluetooth: Off", "class": "off"})
            } else if let Some(dev) = snap
                .connected()
                .find(|d| d.is_audio)
                .or_else(|| snap.connected().next())
            {
                // Prefer the headset over, say, an always-connected mouse.
                let battery = dev
                    .battery
                    .map(|b| format!(" ({}%)", b))
                    .unwrap_or_default();
                serde_json::json!({
                    "text": "󰂱",
                    "tooltip": format!("Bluetooth: Connected to {}{}", dev.name, battery),
                    "class": "connected"
                })
            } else {
                serde_json::json!({"text": "󰂯", "tooltip": "Bluetooth: Disconnected", "class": "disconnected"})
            };
            println!("{}", out);
            Ok(())
        }
        "connect" => run_action(
            args,
            "connect",
            "Connecting to",
            "Connected to",
            autoconnect::manual_connect,
        ),
        "disconnect" => run_action(
            args,
            "disconnect",
            "Disconnecting",
            "Disconnected",
            autoconnect::manual_disconnect,
        ),
        "pair" => run_action(args, "pair", "Pairing with", "Paired with", |mac| {
            autoconnect::unsuppress(mac);
            bt_backend::pair(mac)
        }),
        "trust" => run_action(args, "trust", "Trusting", "Trusted", |mac| {
            bt_backend::trust(mac, true)
        }),
        "remove" | "unpair" => run_action(args, "remove", "Removing", "Removed", |mac| {
            autoconnect::unsuppress(mac);
            bt_backend::remove(mac)
        }),
        "daemon" | "--daemon" | "-d" => autoconnect::run_daemon(),
        "status" => {
            let snap = bt_backend::Snapshot::read().with_audio_profiles();
            let connected: Vec<_> = snap.connected().collect();
            println!(
                "{}",
                serde_json::json!({"powered": snap.powered(), "connected_devices": connected})
            );
            Ok(())
        }
        "fullstatus" => {
            let snap = bt_backend::Snapshot::read().with_audio_profiles();
            let connected: Vec<_> = snap.connected().collect();
            println!(
                "{}",
                serde_json::json!({
                    "status": {"powered": snap.powered(), "connected_devices": connected},
                    "all_devices": snap.devices,
                })
            );
            Ok(())
        }
        "toggle" => {
            let on = !bt_backend::is_powered();
            bt_backend::set_power(on)
                .map(|()| println!("Bluetooth power toggled {}", if on { "ON" } else { "OFF" }))
                .map_err(|e| format!("Failed to toggle Bluetooth power: {}", e))
        }
        "--help" | "-h" | "help" => {
            println!("{}", USAGE);
            Ok(())
        }
        other => {
            eprintln!("{}", USAGE);
            Err(format!("Error: Unknown command '{}'", other))
        }
    };
    Some(result)
}

fn main() -> Result<(), slint::PlatformError> {
    let args: Vec<String> = std::env::args().collect();
    if let Some(result) = handle_commands(&args) {
        if let Err(e) = result {
            eprintln!("{}", e);
            std::process::exit(1);
        }
        return Ok(());
    }

    let main_window = AppWindow::new()?;
    // Matches StartupWMClass in auralink-bt.desktop; without it the window
    // has no app id, so docks and compositor window rules cannot match it.
    let _ = slint::set_xdg_app_id("auralink-bt");
    let config = AppConfig::load(CONFIG_FILE);
    let pywal_enabled = Arc::new(AtomicBool::new(config.pywal_enabled));
    main_window.set_pywal_enabled(config.pywal_enabled);
    if config.pywal_enabled
        && let Some(theme) = Theme::from_pywal()
    {
        set_palette(&main_window, &theme);
    }

    let window_weak = main_window.as_weak();
    main_window.on_toggle_settings(move || {
        if let Some(ui) = window_weak.upgrade() {
            ui.set_show_settings(!ui.get_show_settings());
        }
    });

    let window_weak = main_window.as_weak();
    let pw = pywal_enabled.clone();
    main_window.on_toggle_pywal(move |enabled| {
        pw.store(enabled, Ordering::Relaxed);
        AppConfig {
            pywal_enabled: enabled,
        }
        .save(CONFIG_FILE);
        let theme = if enabled { Theme::from_pywal() } else { None };
        apply_theme(
            window_weak.clone(),
            theme.unwrap_or_else(Theme::default_dark),
        );
    });

    let window_weak = main_window.as_weak();
    common::spawn_pywal_watch(pywal_enabled, move |theme| {
        apply_theme(window_weak.clone(), theme)
    });

    let window_weak = main_window.as_weak();
    main_window.on_toggle_bluetooth(move |enabled| {
        let ww = window_weak.clone();
        std::thread::spawn(move || {
            if let Err(e) = bt_backend::set_power(enabled) {
                set_status(
                    &ww,
                    format!("Power {} failed: {}", if enabled { "on" } else { "off" }, e),
                );
            }
        });
    });

    let window_weak = main_window.as_weak();
    main_window.on_connect(move |address| {
        let address = address.to_string();
        let ww = window_weak.clone();
        set_status(&ww, format!("Connecting to {}...", address));
        std::thread::spawn(move || match autoconnect::manual_connect(&address) {
            Ok(()) => {
                set_status(&ww, format!("Connected to {}", address));
                notify(
                    "Bluetooth Connected",
                    &format!("Connected to {}", address),
                    Some("bluetooth-active"),
                );
            }
            Err(e) => {
                set_status(&ww, format!("Connect failed: {}", e));
                notify(
                    "Bluetooth Connection Error",
                    &format!("Failed to connect to {}: {}", address, e),
                    Some("dialog-error"),
                );
            }
        });
    });

    let window_weak = main_window.as_weak();
    main_window.on_disconnect(move |address| {
        let address = address.to_string();
        let ww = window_weak.clone();
        std::thread::spawn(move || match autoconnect::manual_disconnect(&address) {
            Ok(()) => {
                set_status(&ww, format!("Disconnected {}", address));
                notify(
                    "Bluetooth Disconnected",
                    &format!("Disconnected {}", address),
                    Some("bluetooth-disabled"),
                );
            }
            Err(e) => {
                set_status(&ww, format!("Disconnect failed: {}", e));
                notify(
                    "Bluetooth Error",
                    &format!("Failed to disconnect {}: {}", address, e),
                    Some("dialog-error"),
                );
            }
        });
    });

    let window_weak = main_window.as_weak();
    main_window.on_toggle_trust(move |address, trust| {
        let address = address.to_string();
        let ww = window_weak.clone();
        std::thread::spawn(move || {
            let msg = match bt_backend::trust(&address, trust) {
                Ok(()) => format!(
                    "{} {}",
                    if trust { "Trusted" } else { "Untrusted" },
                    address
                ),
                Err(e) => format!("Trust change failed for {}: {}", address, e),
            };
            set_status(&ww, msg);
        });
    });

    let window_weak = main_window.as_weak();
    main_window.on_toggle_pair(move |address, pair| {
        let address = address.to_string();
        let ww = window_weak.clone();
        std::thread::spawn(move || {
            autoconnect::unsuppress(&address);
            let result = if pair {
                bt_backend::pair(&address)
            } else {
                bt_backend::remove(&address)
            };
            let verb = if pair { "Paired" } else { "Removed" };
            set_status(
                &ww,
                match result {
                    Ok(()) => format!("{} {}", verb, address),
                    Err(e) => format!(
                        "Failed to {} {}: {}",
                        if pair { "pair" } else { "unpair" },
                        address,
                        e
                    ),
                },
            );
        });
    });

    let window_weak = main_window.as_weak();
    main_window.on_remove_device(move |address| {
        let address = address.to_string();
        let ww = window_weak.clone();
        std::thread::spawn(move || {
            autoconnect::unsuppress(&address);
            if let Err(e) = bt_backend::remove(&address) {
                set_status(&ww, format!("Remove failed for {}: {}", address, e));
            }
        });
    });

    let window_weak = main_window.as_weak();
    main_window.on_select_audio_profile(move |address, profile| {
        let address = address.to_string();
        let profile = profile.to_string();
        let ww = window_weak.clone();
        std::thread::spawn(move || {
            let ok = bt_backend::set_audio_profile(&address, &profile);
            set_status(
                &ww,
                if ok {
                    format!("Profile set: {}", profile)
                } else {
                    "Profile set failed".to_string()
                },
            );
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
        let handle = window_weak.clone();
        // The refresh button is an explicit request to scan, so switching
        // the adapter on for it is what the user wants.
        std::thread::spawn(move || discover_devices(handle, true));
    });

    // Look for new devices on open, but only when that is free: opening the
    // window must not undo Bluetooth-off or airplane mode, and discovery
    // makes a connected A2DP stream stutter.
    let window_weak = main_window.as_weak();
    std::thread::spawn(move || {
        let snap = bt_backend::Snapshot::read();
        if snap.powered() && !snap.connected().any(|d| d.is_audio) {
            discover_devices(window_weak, false);
        }
    });

    // Auto-connect, for when the window runs without the daemon (e.g. no
    // systemd unit installed). Re-checks every pass whether the daemon is
    // active, so starting or stopping the unit hands the job over either way.
    let window_weak = main_window.as_weak();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(2));
        let _ = bt_backend::ensure_switch_on_connect_module();
        let mut connector = autoconnect::AutoConnector::new();
        let mut deferring: Option<bool> = None;
        loop {
            let defer = autoconnect::daemon_active();
            if deferring != Some(defer) {
                eprintln!(
                    "{}",
                    if defer {
                        "Auto-connect deferred to auralink-bt-daemon.service."
                    } else {
                        "auralink-bt-daemon.service is not running; this window will auto-connect."
                    }
                );
                deferring = Some(defer);
            }
            let wait = if defer {
                Duration::from_secs(10)
            } else {
                connector.tick(&mut |msg| set_status(&window_weak, msg))
            };
            std::thread::sleep(wait);
        }
    });

    // Device list refresh.
    let window_weak = main_window.as_weak();
    std::thread::spawn(move || {
        loop {
            let snap = bt_backend::Snapshot::read().with_audio_profiles();
            let powered = snap.powered();
            let devices = snap.devices;
            let ww = window_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ww.upgrade() {
                    let items: Vec<BluetoothDevice> = devices.into_iter().map(to_slint).collect();
                    ui.set_devices(ModelRc::new(VecModel::from(items)));
                    ui.set_bluetooth_enabled(powered);
                }
            });
            std::thread::sleep(Duration::from_secs(5));
        }
    });

    let result = main_window.run();
    // Closing the window mid-scan would otherwise orphan the bluetoothctl
    // child, which keeps discovering until its own 60s timeout.
    bt_backend::stop_scan();
    result
}

/// Run a 15s discovery session, showing it in the window. Blocking: call it
/// from a worker thread.
fn discover_devices(window: slint::Weak<AppWindow>, power_on: bool) {
    let ww = window.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ww.upgrade() {
            ui.set_is_scanning(true);
            ui.set_status_msg("SCANNING...".into());
        }
    });
    if power_on && !bt_backend::is_powered() {
        let _ = bt_backend::set_power(true);
    }
    bt_backend::start_scan();
    std::thread::sleep(Duration::from_secs(15));
    bt_backend::stop_scan();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = window.upgrade() {
            ui.set_is_scanning(false);
            ui.set_status_msg("READY".into());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_args_opens_gui() {
        assert!(handle_commands(&args(&["auralink-bt"])).is_none());
        assert!(handle_commands(&args(&["auralink-bt", "popup"])).is_none());
    }

    #[test]
    fn help_succeeds() {
        assert!(matches!(
            handle_commands(&args(&["auralink-bt", "--help"])),
            Some(Ok(()))
        ));
    }

    #[test]
    fn status_commands_are_handled() {
        // These read live BlueZ state; with no BlueZ they report it empty.
        assert!(matches!(
            handle_commands(&args(&["auralink-bt", "status"])),
            Some(Ok(()))
        ));
        assert!(matches!(
            handle_commands(&args(&["auralink-bt", "fullstatus"])),
            Some(Ok(()))
        ));
    }

    #[test]
    fn unknown_command_fails() {
        assert!(matches!(
            handle_commands(&args(&["auralink-bt", "unknown"])),
            Some(Err(_))
        ));
    }

    #[test]
    fn device_commands_need_a_valid_mac() {
        // Missing argument: usage error, nothing touched.
        assert!(matches!(
            handle_commands(&args(&["auralink-bt", "connect"])),
            Some(Err(_))
        ));
        // Malformed address is rejected before any D-Bus call.
        let res = handle_commands(&args(&["auralink-bt", "trust", "not-a-mac"]));
        assert!(matches!(res, Some(Err(e)) if e.contains("not a Bluetooth address")));
    }
}
