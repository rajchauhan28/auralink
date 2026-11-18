mod bt_backend;
mod common;
mod nm_backend;
mod nm_dbus;

use common::{AppConfig, Theme};
use nm_backend::Rescan;
use rfd::FileDialog;
use serde_json::json;
use slint::{Color, ComponentHandle, ModelRc, VecModel};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

mod ui {
    include!(concat!(env!("OUT_DIR"), "/wifi.rs"));
}
use ui::*;

const CONFIG_FILE: &str = "config.json";
/// How often the window asks NetworkManager for a fresh scan while open.
const RESCAN_EVERY: Duration = Duration::from_secs(60);
/// How often the network and VPN lists are re-read from NetworkManager.
const LIST_EVERY: Duration = Duration::from_secs(10);

/// Wake the list thread for a fresh Wi-Fi scan (the refresh button).
static RESCAN_REQUESTED: AtomicBool = AtomicBool::new(false);
/// Wake the list thread to re-read the lists and radio state from cache,
/// after an action changed them, so the window does not lag up to
/// LIST_EVERY behind what the user just did.
static RELIST_REQUESTED: AtomicBool = AtomicBool::new(false);

fn request_relist() {
    RELIST_REQUESTED.store(true, Ordering::SeqCst);
}

fn notify(summary: &str, body: &str, icon: Option<&str>) {
    common::send_notification("AuraLink", summary, body, icon);
}

fn set_palette(ui: &WifiWindow, t: &Theme) {
    let p = ui.global::<WifiPalette>();
    p.set_background(t.background);
    p.set_card_bg(t.card_bg);
    p.set_accent(t.accent);
    p.set_foreground(t.foreground);
    p.set_secondary_fg(t.secondary_fg);
    p.set_separator(t.separator);
    p.set_danger(t.danger);
    p.set_upload(t.upload);
}

/// Hop to the UI thread and apply a theme.
fn apply_theme(handle: slint::Weak<WifiWindow>, theme: Theme) {
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = handle.upgrade() {
            set_palette(&ui, &theme);
        }
    });
}

/// Hop to the UI thread and set the status line.
fn set_status(handle: &slint::Weak<WifiWindow>, msg: String) {
    let handle = handle.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = handle.upgrade() {
            ui.set_status_msg(msg.into());
        }
    });
}

const USAGE: &str = "\
Usage: auralink [COMMAND]

Commands:
  popup                Open the Wi-Fi GUI interface
  connect <SSID> [PAS] Connect to a Wi-Fi network
  disconnect           Disconnect active Wi-Fi
  toggle               Toggle Wi-Fi radio on/off
  rescan               Trigger a Wi-Fi network scan
  status               Get current connection status (JSON)
  fullstatus           Get detailed status including available networks (JSON)
  waybar-stream        Print a Waybar module status line (JSON)
  --help               Show this help message";

/// The `status` object, shared by `status` and `fullstatus`.
fn status_json(
    networks: &[nm_backend::Network],
    vpns: Vec<nm_backend::VpnConnection>,
) -> serde_json::Value {
    let active_network = networks.iter().find(|n| n.connected);
    let active_vpns: Vec<_> = vpns.into_iter().filter(|v| v.active).collect();
    let (rx, tx) = nm_backend::get_active_interface()
        .and_then(|iface| nm_backend::get_interface_stats(&iface))
        .unwrap_or((0, 0));
    json!({
        "active_network": active_network,
        "active_vpns": active_vpns,
        "interface_stats": {"rx": rx, "tx": tx},
        "is_scanning": false,
    })
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
        "connect" => match args.get(2) {
            None => Err("Usage: auralink connect <SSID> [password]".to_string()),
            Some(ssid) => {
                println!("Connecting to {}...", ssid);
                nm_backend::connect_to_wifi(ssid, args.get(3).map(String::as_str))
                    .map(|()| println!("Successfully connected to {}", ssid))
                    .map_err(|e| format!("Failed to connect to {}: {}", ssid, e))
            }
        },
        "disconnect" => {
            println!("Disconnecting Wi-Fi...");
            nm_backend::disconnect_wifi()
                .map(|()| println!("Disconnected successfully"))
                .map_err(|e| format!("Disconnect failed: {}", e))
        }
        "toggle" => {
            let on = !nm_backend::is_wifi_enabled();
            if nm_backend::toggle_wifi_power(on) {
                println!("Wi-Fi radio toggled {}", if on { "ON" } else { "OFF" });
                Ok(())
            } else {
                Err("Failed to toggle Wi-Fi radio".to_string())
            }
        }
        "rescan" | "scan" => {
            println!("Triggering Wi-Fi rescan...");
            nm_backend::trigger_rescan();
            println!("Rescan triggered");
            Ok(())
        }
        "waybar-stream" => {
            let out = match nm_backend::list_networks(Rescan::Cached)
                .iter()
                .find(|n| n.connected)
            {
                Some(net) => json!({
                    "text": "󰤨",
                    "tooltip": format!("Wi-Fi: Connected to {}", net.ssid),
                    "class": "connected"
                }),
                None => {
                    json!({"text": "󰤩", "tooltip": "Wi-Fi: Disconnected", "class": "disconnected"})
                }
            };
            println!("{}", out);
            Ok(())
        }
        "status" => {
            // Cached scan results: only the active connection is reported.
            let (networks, vpns) = nm_backend::list_networks_and_vpns(Rescan::Cached);
            println!("{}", status_json(&networks, vpns));
            Ok(())
        }
        "fullstatus" => {
            // Lists available networks, so let nmcli refresh a stale scan.
            let (networks, vpns) = nm_backend::list_networks_and_vpns(Rescan::IfStale);
            let status = status_json(&networks, vpns);
            println!(
                "{}",
                json!({"status": status, "available_networks": networks})
            );
            Ok(())
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

fn network_item(n: nm_backend::Network) -> WifiNetworkItem {
    let (icon, icon_color) = if n.is_ethernet {
        ("󰈀", Color::from_rgb_u8(46, 194, 126))
    } else if n.signal > 75 {
        ("󰤨", Color::from_rgb_u8(46, 194, 126))
    } else if n.signal > 50 {
        ("󰤥", Color::from_rgb_u8(245, 194, 17))
    } else if n.signal > 25 {
        ("󰤢", Color::from_rgb_u8(255, 165, 0))
    } else {
        ("󰤟", Color::from_rgb_u8(237, 51, 59))
    };
    WifiNetworkItem {
        ssid: n.ssid.into(),
        signal: n.signal as i32,
        security: n.security.into(),
        connected: n.connected,
        saved: n.saved,
        ping: n
            .ping
            .map(|p| format!("{} ms", p))
            .unwrap_or_default()
            .into(),
        icon: icon.into(),
        icon_color,
        is_ethernet: n.is_ethernet,
    }
}

/// Which radios are on. Airplane mode is "everything off", not a separate
/// switch in the system, so it is derived rather than stored.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Radios {
    wifi: bool,
    has_bluetooth: bool,
    bluetooth: bool,
}

impl Radios {
    fn read() -> Self {
        let bt = bt_backend::Snapshot::read();
        Radios {
            wifi: nm_backend::is_wifi_enabled(),
            has_bluetooth: !bt.adapters.is_empty(),
            bluetooth: bt.powered(),
        }
    }

    fn airplane_mode(self) -> bool {
        !self.wifi && !(self.has_bluetooth && self.bluetooth)
    }
}

/// Read the radio state (blocking) and hand it to the window.
fn publish_radios(window: &slint::Weak<WifiWindow>) -> Radios {
    let radios = Radios::read();
    let ww = window.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ww.upgrade() {
            ui.set_wifi_enabled(radios.wifi);
            ui.set_has_bluetooth(radios.has_bluetooth);
            ui.set_bluetooth_enabled(radios.bluetooth);
            ui.set_airplane_mode(radios.airplane_mode());
        }
    });
    radios
}

/// Read the network and VPN lists (blocking) and hand them to the window.
fn publish_networks(window: &slint::Weak<WifiWindow>, rescan: Rescan) {
    let scanning = rescan == Rescan::Fresh;
    if scanning {
        let ww = window.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ww.upgrade() {
                ui.set_is_scanning(true);
            }
        });
    }
    let (mut networks, vpns) = nm_backend::list_networks_and_vpns(rescan);
    for net in networks.iter_mut().filter(|n| n.connected) {
        net.ping = nm_backend::get_ping("8.8.8.8");
    }
    let ww = window.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ww.upgrade() {
            let items: Vec<WifiNetworkItem> = networks.into_iter().map(network_item).collect();
            let vpn_items: Vec<WifiVpnItem> = vpns
                .into_iter()
                .map(|v| WifiVpnItem {
                    name: v.name.into(),
                    active: v.active,
                    type_name: v.vpn_type.into(),
                })
                .collect();
            ui.set_networks(ModelRc::new(VecModel::from(items)));
            ui.set_vpns(ModelRc::new(VecModel::from(vpn_items)));
            if scanning {
                ui.set_is_scanning(false);
            }
        }
    });
}

/// How a terminal emulator takes the command to run.
const TERMINALS: &[(&str, &[&str])] = &[
    ("xdg-terminal-exec", &[]),
    ("kitty", &[]),
    ("foot", &[]),
    ("ghostty", &["-e"]),
    ("alacritty", &["-e"]),
    ("wezterm", &["start", "--"]),
    ("konsole", &["-e"]),
    ("gnome-terminal", &["--"]),
    ("xfce4-terminal", &["-x"]),
    ("xterm", &["-e"]),
];

/// Choose a terminal: `$TERMINAL` when it is installed, else the first known
/// one that is. Returns the program and the arguments that precede the
/// command.
fn pick_terminal(
    preferred: Option<&str>,
    installed: impl Fn(&str) -> bool,
) -> Option<(String, &'static [&'static str])> {
    let args_for = |program: &str| {
        let name = std::path::Path::new(program)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(program);
        TERMINALS
            .iter()
            .find(|(t, _)| *t == name)
            .map_or(&["-e"][..], |(_, a)| *a)
    };
    preferred
        .filter(|t| !t.is_empty() && installed(t))
        .map(|t| (t.to_string(), args_for(t)))
        .or_else(|| {
            TERMINALS
                .iter()
                .find(|(t, _)| installed(t))
                .map(|(t, a)| (t.to_string(), *a))
        })
}

fn on_path(program: &str) -> bool {
    if program.contains('/') {
        return std::path::Path::new(program).is_file();
    }
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|dir| dir.join(program).is_file()))
}

/// Run a terminal program such as nmtui in a terminal emulator. A GUI has no
/// terminal of its own: spawning nmtui directly (the old "Advanced Editor"
/// button) did nothing visible.
fn open_in_terminal(command: &str) -> Result<(), String> {
    let preferred = std::env::var("TERMINAL").ok();
    let (term, args) = pick_terminal(preferred.as_deref(), on_path)
        .ok_or("no terminal emulator found; set $TERMINAL")?;
    let mut child = std::process::Command::new(&term)
        .args(args)
        .arg(command)
        .spawn()
        .map_err(|e| format!("{}: {}", term, e))?;
    // Reap it when it exits so it does not linger as a zombie.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
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

    let main_window = WifiWindow::new()?;
    // Matches StartupWMClass in auralink.desktop; without it the window has
    // no app id, so docks and compositor window rules cannot match it.
    let _ = slint::set_xdg_app_id("auralink");
    let config = AppConfig::load(CONFIG_FILE);
    let pywal_enabled = Arc::new(AtomicBool::new(config.pywal_enabled));

    main_window.set_pywal_enabled(config.pywal_enabled);
    if config.pywal_enabled
        && let Some(theme) = Theme::from_pywal()
    {
        set_palette(&main_window, &theme);
    }

    main_window.on_refresh(|| RESCAN_REQUESTED.store(true, Ordering::SeqCst));

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
    main_window.on_toggle_settings(move || {
        if let Some(ui) = window_weak.upgrade() {
            ui.set_show_settings(!ui.get_show_settings());
        }
    });

    let window_weak = main_window.as_weak();
    main_window.on_open_nmtui(move || {
        if let Err(e) = open_in_terminal("nmtui") {
            set_status(&window_weak, format!("Cannot open nmtui: {}", e));
        }
    });

    let window_weak = main_window.as_weak();
    main_window.on_toggle_wifi(move |on| {
        let ww = window_weak.clone();
        std::thread::spawn(move || {
            let state = if on { "on" } else { "off" };
            if nm_backend::toggle_wifi_power(on) {
                set_status(&ww, format!("Wi-Fi {}", state));
            } else {
                set_status(&ww, format!("Could not turn Wi-Fi {}", state));
            }
            request_relist();
            if on {
                // The radio takes a few seconds to come up and accept a scan.
                std::thread::sleep(Duration::from_secs(4));
                RESCAN_REQUESTED.store(true, Ordering::SeqCst);
            }
        });
    });

    let window_weak = main_window.as_weak();
    main_window.on_toggle_bluetooth(move |on| {
        let ww = window_weak.clone();
        std::thread::spawn(move || {
            let state = if on { "on" } else { "off" };
            match bt_backend::set_power(on) {
                Ok(()) => set_status(&ww, format!("Bluetooth {}", state)),
                Err(e) => set_status(&ww, format!("Could not turn Bluetooth {}: {}", state, e)),
            }
            request_relist();
        });
    });

    let window_weak = main_window.as_weak();
    main_window.on_disconnect(move || {
        let ww = window_weak.clone();
        // nmcli runs on the worker thread; calling it on the UI thread froze
        // the window until NetworkManager answered.
        std::thread::spawn(move || {
            let result = nm_backend::disconnect_wifi();
            request_relist();
            match result {
                Ok(()) => {
                    set_status(&ww, "Disconnected".to_string());
                    notify(
                        "Wi-Fi Disconnected",
                        "Disconnected successfully",
                        Some("network-wireless-disconnected"),
                    );
                }
                Err(e) => {
                    set_status(&ww, format!("Disconnect failed: {}", e));
                    notify(
                        "Wi-Fi Error",
                        &format!("Disconnect failed: {}", e),
                        Some("dialog-error"),
                    );
                }
            }
        });
    });

    let window_weak = main_window.as_weak();
    main_window.on_forget_network(move |ssid| {
        let ssid = ssid.to_string();
        let ww = window_weak.clone();
        std::thread::spawn(move || {
            let forgot = nm_backend::forget_network(&ssid);
            request_relist();
            if forgot {
                set_status(&ww, format!("Forgot {}", ssid));
                notify(
                    "Wi-Fi Network",
                    &format!("Forgot network: {}", ssid),
                    Some("user-trash"),
                );
            } else {
                set_status(&ww, format!("Failed to forget {}", ssid));
                notify(
                    "Wi-Fi Error",
                    &format!("Failed to forget network: {}", ssid),
                    Some("dialog-error"),
                );
            }
        });
    });

    let window_weak = main_window.as_weak();
    main_window.on_show_network_info(move |ssid| {
        let ssid = ssid.to_string();
        let ww = window_weak.clone();
        std::thread::spawn(move || {
            let info = nm_backend::get_network_info(&ssid);
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ww.upgrade() {
                    ui.set_info_content(info.into());
                    ui.set_show_info(true);
                }
            });
        });
    });

    let window_weak = main_window.as_weak();
    main_window.on_show_network_advanced(move |ssid| {
        let ssid = ssid.to_string();
        let ww = window_weak.clone();
        std::thread::spawn(move || {
            let cfg_opt = nm_backend::get_network_config(&ssid);
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ww.upgrade() else { return };
                let Some(cfg) = cfg_opt else {
                    ui.set_status_msg("Could not load config".into());
                    return;
                };
                ui.set_active_config_ssid(ssid.into());
                ui.set_config_autoconnect(cfg.autoconnect);
                ui.set_config_priority(cfg.priority);
                ui.set_config_dns(cfg.dns.into());
                ui.set_config_ipv4_method(cfg.ipv4_method.into());
                ui.set_config_ipv4_address(cfg.ipv4_address.into());
                ui.set_config_ipv4_gateway(cfg.ipv4_gateway.into());
                ui.set_config_ipv6_method(cfg.ipv6_method.into());
                ui.set_config_ipv6_address(cfg.ipv6_address.into());
                ui.set_config_ipv6_gateway(cfg.ipv6_gateway.into());
                ui.set_config_mac_address(cfg.mac_address.into());
                ui.set_config_password(cfg.password.into());
                ui.set_show_advanced(true);
            });
        });
    });

    let window_weak = main_window.as_weak();
    main_window.on_save_network_config(
        move |ssid, auto, prio, dns, i4m, i4a, i4g, i6m, i6a, i6g, mac, pass| {
            let ssid = ssid.to_string();
            let config = nm_backend::NetworkConfig {
                autoconnect: auto,
                priority: prio,
                dns: dns.to_string(),
                ipv4_method: i4m.to_string(),
                ipv4_address: i4a.to_string(),
                ipv4_gateway: i4g.to_string(),
                ipv6_method: i6m.to_string(),
                ipv6_address: i6a.to_string(),
                ipv6_gateway: i6g.to_string(),
                mac_address: mac.to_string(),
                password: pass.to_string(),
            };
            let ww = window_weak.clone();
            std::thread::spawn(move || {
                let result = nm_backend::update_network_config(&ssid, config);
                request_relist();
                match result {
                    Ok(()) => {
                        set_status(&ww, "Settings saved".to_string());
                        notify(
                            "Wi-Fi Configuration",
                            &format!("Settings saved for {}", ssid),
                            Some("network-wireless-encrypted"),
                        );
                    }
                    Err(e) => {
                        set_status(&ww, format!("Save failed: {}", e));
                        notify(
                            "Wi-Fi Error",
                            &format!("Failed to save settings for {}: {}", ssid, e),
                            Some("dialog-error"),
                        );
                    }
                }
            });
        },
    );

    let window_weak = main_window.as_weak();
    main_window.on_expand_network(move |ssid| {
        if let Some(ui) = window_weak.upgrade() {
            ui.set_expanded_ssid(ssid);
        }
    });

    let window_weak = main_window.as_weak();
    main_window.on_connect(move |ssid, pass| {
        let ssid = ssid.to_string();
        let password = (!pass.is_empty()).then(|| pass.to_string());
        let Some(ui) = window_weak.upgrade() else {
            return;
        };
        ui.set_is_connecting(true);
        ui.set_status_msg(format!("Connecting to {}...", ssid).into());

        let ww = window_weak.clone();
        std::thread::spawn(move || {
            let result = nm_backend::connect_to_wifi(&ssid, password.as_deref());
            request_relist();
            let msg = match &result {
                Ok(()) => format!("Connected to {}", ssid),
                Err(e) => format!("Failed: {}", e),
            };
            let handle = ww.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = handle.upgrade() {
                    ui.set_is_connecting(false);
                    ui.set_status_msg(msg.into());
                }
            });
            match result {
                Ok(()) => notify(
                    "Wi-Fi Connected",
                    &format!("Connected to {}", ssid),
                    Some("network-wireless-connected"),
                ),
                Err(e) => notify(
                    "Wi-Fi Connection Error",
                    &format!("Failed to connect to {}: {}", ssid, e),
                    Some("network-wireless-offline"),
                ),
            }
        });
    });

    let window_weak = main_window.as_weak();
    main_window.on_toggle_vpn(move |name, vtype, active| {
        let name = name.to_string();
        let vtype = vtype.to_string();
        let ww = window_weak.clone();
        std::thread::spawn(move || {
            let ok = nm_backend::toggle_vpn(&name, &vtype, active);
            // The switch flipped itself; the relist puts it back if this failed.
            request_relist();
            if ok {
                notify(
                    "VPN Status",
                    &format!("VPN {} toggled {}", name, if active { "ON" } else { "OFF" }),
                    Some("security-high"),
                );
            } else {
                set_status(&ww, format!("VPN Toggle Failed for {}", name));
                notify(
                    "VPN Error",
                    &format!("Failed to toggle VPN: {}", name),
                    Some("dialog-error"),
                );
            }
        });
    });

    let window_weak = main_window.as_weak();
    main_window.on_toggle_airplane_mode(move |enabled| {
        let ww = window_weak.clone();
        std::thread::spawn(move || {
            let wifi_ok = nm_backend::toggle_wifi_power(!enabled);
            // A machine without Bluetooth has nothing to switch.
            let bt_ok = bt_backend::set_power(!enabled).is_ok() || !bt_backend::has_adapter();
            request_relist();
            let state = if enabled { "ON" } else { "OFF" };
            // Both radios have to follow; "either one" reported a half-applied
            // airplane mode as success.
            if wifi_ok && bt_ok {
                set_status(&ww, format!("Airplane Mode: {}", state));
                notify(
                    "Airplane Mode",
                    &format!("Airplane Mode: {}", state),
                    Some("airplane-mode"),
                );
            } else {
                let failed = match (wifi_ok, bt_ok) {
                    (false, false) => "Wi-Fi and Bluetooth",
                    (false, true) => "Wi-Fi",
                    _ => "Bluetooth",
                };
                set_status(&ww, format!("Airplane Mode: could not switch {}", failed));
                notify(
                    "Airplane Mode Error",
                    &format!("Could not switch {} radio", failed),
                    Some("dialog-error"),
                );
            }
        });
    });

    let window_weak = main_window.as_weak();
    main_window.on_import_vpn(move || {
        let ww = window_weak.clone();
        // The picker blocks until the user chooses; on the UI thread that
        // froze the window. rfd's portal backend works from any thread.
        std::thread::spawn(move || {
            let Some(file) = FileDialog::new()
                .add_filter("VPN Config", &["conf", "ovpn"])
                .pick_file()
            else {
                return;
            };
            let name = file
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if nm_backend::import_vpn(&file.display().to_string()) {
                set_status(&ww, format!("Imported {}", name));
                request_relist();
            } else {
                set_status(&ww, format!("Could not import {}", name));
            }
        });
    });

    // Network and VPN lists.
    let window_weak = main_window.as_weak();
    std::thread::spawn(move || {
        // Show the cached list at once; it can be minutes stale (after a
        // resume or a move it may hold only the network in use), so follow
        // it with a fresh scan. Rescan again every RESCAN_EVERY and when the
        // user refreshes. A fresh scan blocks for seconds, hence this thread.
        publish_radios(&window_weak);
        publish_networks(&window_weak, Rescan::Cached);
        let mut last_rescan: Option<Instant> = None;
        loop {
            // swap() both first: behind a short-circuiting || a request could
            // stay set and run the loop twice.
            let forced = RESCAN_REQUESTED.swap(false, Ordering::SeqCst);
            RELIST_REQUESTED.store(false, Ordering::SeqCst);
            let radios = publish_radios(&window_weak);
            let due = forced || last_rescan.is_none_or(|t| t.elapsed() >= RESCAN_EVERY);
            // Scanning with the radio off fails anyway; leave that for the
            // first pass after it is switched back on.
            let rescan = if due && radios.wifi {
                last_rescan = Some(Instant::now());
                Rescan::Fresh
            } else {
                Rescan::Cached
            };
            publish_networks(&window_weak, rescan);

            // Sleep, waking early for a refresh or after an action.
            let until = Instant::now() + LIST_EVERY;
            while Instant::now() < until
                && !RESCAN_REQUESTED.load(Ordering::SeqCst)
                && !RELIST_REQUESTED.load(Ordering::SeqCst)
            {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    });

    // Throughput graph.
    let window_weak = main_window.as_weak();
    std::thread::spawn(move || {
        let mut last_stats: Option<(u64, u64, Instant)> = None;
        let mut download_history = VecDeque::from(vec![0.0f32; 60]);
        let mut upload_history = VecDeque::from(vec![0.0f32; 60]);
        let mut active_iface: Option<String> = None;
        let mut last_iface_check: Option<Instant> = None;

        loop {
            if last_iface_check.is_none_or(|t| t.elapsed() > Duration::from_secs(5)) {
                let iface = nm_backend::get_active_interface();
                if iface != active_iface {
                    // Counters belong to an interface. Diffing the new one's
                    // against the old one's wrapped around to a ~10^19 B/s
                    // "speed" that flattened the graph for a whole minute.
                    last_stats = None;
                    active_iface = iface;
                }
                last_iface_check = Some(Instant::now());
            }

            if let Some(ref iface) = active_iface
                && let Some((rx, tx)) = nm_backend::get_interface_stats(iface)
            {
                let now = Instant::now();
                match last_stats {
                    Some((l_rx, l_tx, l_time))
                        if now.duration_since(l_time).as_secs_f64() > 0.1 =>
                    {
                        let secs = now.duration_since(l_time).as_secs_f64();
                        // saturating: counters reset when an interface is
                        // re-created, which is not negative traffic.
                        let rx_speed = rx.saturating_sub(l_rx) as f64 / secs;
                        let tx_speed = tx.saturating_sub(l_tx) as f64 / secs;

                        download_history.pop_front();
                        download_history.push_back(rx_speed as f32);
                        upload_history.pop_front();
                        upload_history.push_back(tx_speed as f32);
                        let max = download_history
                            .iter()
                            .chain(&upload_history)
                            .fold(1.0f32, |a, &b| a.max(b));

                        let dl: Vec<f32> = download_history.iter().copied().collect();
                        let ul: Vec<f32> = upload_history.iter().copied().collect();
                        let (dl_str, ul_str) = (
                            common::format_speed(rx_speed),
                            common::format_speed(tx_speed),
                        );
                        let ww = window_weak.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ww.upgrade() {
                                ui.set_download_history(ModelRc::new(VecModel::from(dl)));
                                ui.set_upload_history(ModelRc::new(VecModel::from(ul)));
                                ui.set_max_speed(max);
                                ui.set_current_download(dl_str.into());
                                ui.set_current_upload(ul_str.into());
                            }
                        });
                        last_stats = Some((rx, tx, now));
                    }
                    Some(_) => {}
                    None => last_stats = Some((rx, tx, now)),
                }
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    });

    main_window.run()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn cli_dispatch() {
        assert!(handle_commands(&args(&["auralink"])).is_none());
        assert!(handle_commands(&args(&["auralink", "popup"])).is_none());
        assert!(matches!(
            handle_commands(&args(&["auralink", "--help"])),
            Some(Ok(()))
        ));
        assert!(matches!(
            handle_commands(&args(&["auralink", "bogus"])),
            Some(Err(_))
        ));
        // Missing SSID is a usage error, and nothing is touched.
        assert!(matches!(
            handle_commands(&args(&["auralink", "connect"])),
            Some(Err(_))
        ));
    }

    #[test]
    fn terminal_prefers_an_installed_terminal_env() {
        let installed = |t: &str| matches!(t, "kitty" | "alacritty" | "/opt/bin/ghostty");
        // $TERMINAL wins when it is installed, with its own argument style.
        assert_eq!(
            pick_terminal(Some("alacritty"), installed),
            Some(("alacritty".to_string(), &["-e"][..]))
        );
        assert_eq!(
            pick_terminal(Some("/opt/bin/ghostty"), installed),
            Some(("/opt/bin/ghostty".to_string(), &["-e"][..]))
        );
        // A stale $TERMINAL (here: not installed) falls back to the list.
        assert_eq!(
            pick_terminal(Some("wezterm"), installed),
            Some(("kitty".to_string(), &[][..]))
        );
        assert_eq!(
            pick_terminal(None, installed),
            Some(("kitty".to_string(), &[][..]))
        );
        assert_eq!(pick_terminal(Some("kitty"), |_| false), None);
    }

    #[test]
    fn airplane_mode_means_every_radio_off() {
        let r = |wifi, has_bluetooth, bluetooth| Radios {
            wifi,
            has_bluetooth,
            bluetooth,
        };
        assert!(r(false, true, false).airplane_mode());
        assert!(!r(true, true, false).airplane_mode());
        assert!(!r(false, true, true).airplane_mode());
        // No Bluetooth hardware: Wi-Fi alone decides.
        assert!(r(false, false, false).airplane_mode());
        assert!(!r(true, false, false).airplane_mode());
    }
}
