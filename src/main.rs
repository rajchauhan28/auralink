mod nm_backend;

use slint::{VecModel, Color, ModelRc, ComponentHandle};
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use std::time::{Duration, Instant};
use std::collections::VecDeque;
use rfd::FileDialog;
use serde_json::json;

mod ui {
    include!(concat!(env!("OUT_DIR"), "/wifi.rs"));
}
use ui::*;

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct AppConfig {
    pywal_enabled: bool,
}

impl AppConfig {
    fn load() -> Self {
        let home = std::env::var("HOME").unwrap_or_default();
        let path = format!("{}/.config/auralink/config.json", home);
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
        let path = format!("{}/config.json", dir);
        let _ = std::fs::create_dir_all(dir);
        if let Ok(s) = serde_json::to_string(self) {
            let _ = std::fs::write(path, s);
        }
    }
}

fn format_speed(bytes_per_sec: f64) -> String {
    if bytes_per_sec >= 1024.0 * 1024.0 { format!("{:.1} MB/s", bytes_per_sec / (1024.0 * 1024.0)) }
    else if bytes_per_sec >= 1024.0 { format!("{:.1} KB/s", bytes_per_sec / 1024.0) }
    else { format!("{:.0} B/s", bytes_per_sec) }
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

fn apply_pywal_theme(handle: slint::Weak<WifiWindow>) {
    let home = std::env::var("HOME").unwrap_or_default();
    let path = format!("{}/.cache/wal/colors.json", home);
    
    if let Ok(content) = std::fs::read_to_string(path) {
        if let Ok(wal) = serde_json::from_str::<PywalColors>(&content) {
            let mut bg = parse_hex(wal.special.get("background").unwrap_or(&"#09090b".to_string())).unwrap_or(Color::from_rgb_u8(9, 9, 11));
            // Add 50% opacity for glassmorphism to the pywal background
            bg = Color::from_argb_u8(136, bg.red(), bg.green(), bg.blue());
            
            let fg = parse_hex(wal.special.get("foreground").unwrap_or(&"#f8fafc".to_string())).unwrap_or(Color::from_rgb_u8(248, 250, 252));
            // Usually color1 or color4 is a good accent
            let accent = parse_hex(wal.colors.get("color1").unwrap_or(&"#00f0ff".to_string())).unwrap_or(Color::from_rgb_u8(0, 240, 255));
            
            let card_bg = Color::from_argb_u8(255, 
                (bg.red() as i16 + 15).clamp(0, 255) as u8,
                (bg.green() as i16 + 15).clamp(0, 255) as u8,
                (bg.blue() as i16 + 15).clamp(0, 255) as u8
            );

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = handle.upgrade() {
                    let palette = ui.global::<WifiPalette>();
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
}

#[tokio::main]
async fn main() -> Result<(), slint::PlatformError> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let cmd = args[1].as_str();
        match cmd {
            "status" => {
                let networks = nm_backend::list_networks();
                let vpns = nm_backend::list_vpns();
                let active_network = networks.iter().find(|n| n.connected).cloned();
                let active_vpns: Vec<_> = vpns.into_iter().filter(|v| v.active).collect();
                let active_iface = nm_backend::get_active_interface();
                let stats = active_iface.as_ref()
                    .and_then(|iface| nm_backend::get_interface_stats(iface))
                    .unwrap_or((0, 0));

                let status = json!({
                    "active_network": active_network,
                    "active_vpns": active_vpns,
                    "interface_stats": {
                        "rx": stats.0,
                        "tx": stats.1,
                    },
                    "is_scanning": false,
                });
                println!("{}", status);
                return Ok(());
            }
            "fullstatus" => {
                let networks = nm_backend::list_networks();
                let vpns = nm_backend::list_vpns();
                let active_network = networks.iter().find(|n| n.connected).cloned();
                let active_vpns: Vec<_> = vpns.into_iter().filter(|v| v.active).collect();
                let active_iface = nm_backend::get_active_interface();
                let stats = active_iface.as_ref()
                    .and_then(|iface| nm_backend::get_interface_stats(iface))
                    .unwrap_or((0, 0));

                let status_obj = json!({
                    "active_network": active_network,
                    "active_vpns": active_vpns,
                    "interface_stats": {
                        "rx": stats.0,
                        "tx": stats.1,
                    },
                    "is_scanning": false,
                });

                let full_status = json!({
                    "status": status_obj,
                    "available_networks": networks,
                });
                println!("{}", full_status);
                return Ok(());
            }
            "--help" | "-h" | "help" => {
                println!("Usage: auralink [COMMAND]\n");
                println!("Commands:");
                println!("  status      Get current connection status (JSON)");
                println!("  fullstatus  Get detailed status including available networks (JSON)");
                println!("  --help      Show this help message");
                return Ok(());
            }
            _ => {
                eprintln!("Error: Unknown command '{}'", cmd);
                println!("Usage: auralink [COMMAND]\n");
                println!("Commands:");
                println!("  status      Get current connection status (JSON)");
                println!("  fullstatus  Get detailed status including available networks (JSON)");
                println!("  --help      Show this help message");
                std::process::exit(1);
            }
        }
    }

    let main_window = WifiWindow::new()?;
    let config = Arc::new(Mutex::new(AppConfig::load()));
    let force_refresh = Arc::new(AtomicBool::new(false));
    
    if let Ok(cfg) = config.lock() {
        main_window.set_pywal_enabled(cfg.pywal_enabled);
        if cfg.pywal_enabled {
            apply_pywal_theme(main_window.as_weak());
        }
    }

    let fr_clone = force_refresh.clone();
    main_window.on_refresh(move || {
        fr_clone.store(true, Ordering::SeqCst);
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
                        let p = ui.global::<WifiPalette>();
                        p.set_background(parse_hex("#09090b88").unwrap());
                        p.set_card_bg(parse_hex("#18181b").unwrap());
                        p.set_accent(parse_hex("#00f0ff").unwrap());
                        p.set_foreground(parse_hex("#f8fafc").unwrap());
                        p.set_secondary_fg(parse_hex("#a1a1aa").unwrap());
                        p.set_separator(parse_hex("#27272a").unwrap());
                    }
                });
            }
        }
    });

    let window_weak = main_window.as_weak();
    main_window.on_toggle_settings(move || {
        if let Some(ui) = window_weak.upgrade() {
            ui.set_show_settings(!ui.get_show_settings());
        }
    });

    main_window.on_open_nmtui(|| {
        let _ = std::process::Command::new("nmtui").spawn();
    });

    main_window.on_disconnect(move || {
        nm_backend::disconnect_wifi();
    });

    let window_weak = main_window.as_weak();
    main_window.on_forget_network(move |ssid| {
        let ssid = ssid.to_string();
        let ww = window_weak.clone();
        std::thread::spawn(move || {
            let success = nm_backend::forget_network(&ssid);
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ww.upgrade() {
                    ui.set_status_msg(if success { format!("Forgot {}", ssid) } else { format!("Failed to forget {}", ssid) }.into());
                }
            });
        });
    });

    let window_weak = main_window.as_weak();
    main_window.on_show_network_info(move |ssid| {
        let ssid = ssid.to_string();
        if let Some(ui) = window_weak.upgrade() {
            let info = nm_backend::get_network_info(&ssid);
            ui.set_info_content(info.into());
            ui.set_show_info(true);
        }
    });

    let window_weak = main_window.as_weak();
    main_window.on_show_network_advanced(move |ssid| {
        let ssid = ssid.to_string();
        if let Some(ui) = window_weak.upgrade() {
            if let Some(cfg) = nm_backend::get_network_config(&ssid) {
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
            } else {
                ui.set_status_msg("Could not load config".into());
            }
        }
    });

    let window_weak = main_window.as_weak();
    main_window.on_save_network_config(move |ssid, auto, prio, dns, i4m, i4a, i4g, i6m, i6a, i6g, mac, pass| {
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
            let success = nm_backend::update_network_config(&ssid, config);
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ww.upgrade() {
                    ui.set_status_msg(if success { "Settings saved".to_string() } else { "Save failed".to_string() }.into());
                }
            });
        });
    });

    let window_weak = main_window.as_weak();
    main_window.on_expand_network(move |ssid| {
        if let Some(ui) = window_weak.upgrade() {
            ui.set_expanded_ssid(ssid);
        }
    });

    let window_weak = main_window.as_weak();
    main_window.on_connect(move |ssid, _pass| {
        let ssid = ssid.to_string();
        if let Some(ui) = window_weak.upgrade() {
            ui.set_is_connecting(true);
            ui.set_status_msg(format!("Connecting to {}...", ssid).into());
            
            let ww = window_weak.clone();
            std::thread::spawn(move || {
                let success = nm_backend::connect_to_wifi(&ssid, None);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ww.upgrade() {
                        ui.set_is_connecting(false);
                        ui.set_status_msg(if success { format!("Connected to {}", ssid) } else { "Failed!".to_string() }.into());
                    }
                });
            });
        }
    });

    let window_weak = main_window.as_weak();
    main_window.on_toggle_vpn(move |name, vtype, active| {
        let name = name.to_string();
        let vtype = vtype.to_string();
        let ww = window_weak.clone();
        std::thread::spawn(move || {
            let success = nm_backend::toggle_vpn(&name, &vtype, active);
            if !success {
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ww.upgrade() {
                        ui.set_status_msg(format!("VPN Toggle Failed for {}", name).into());
                    }
                });
            }
        });
    });

    let window_weak = main_window.as_weak();
    main_window.on_import_vpn(move || {
        if let Some(file) = FileDialog::new()
            .add_filter("VPN Config", &["conf", "ovpn"])
            .pick_file() {
            let path = file.display().to_string();
            let ww = window_weak.clone();
            std::thread::spawn(move || {
                let success = nm_backend::import_vpn(&path);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ww.upgrade() {
                        ui.set_status_msg(if success { "VPN Imported" } else { "Import Failed" }.into());
                    }
                });
            });
        }
    });

    // Background thread for Network & VPN List
    let window_weak = main_window.as_weak();
    let fr_clone = force_refresh.clone();
    std::thread::spawn(move || {
        loop {
            let mut networks = nm_backend::list_networks();
            for net in &mut networks {
                if net.connected {
                    net.ping = nm_backend::get_ping("8.8.8.8");
                }
            }

            let slint_networks: Vec<WifiNetworkItem> = networks.into_iter().map(|n| {
                let (icon, icon_color) = if n.is_ethernet { ("󰈀", Color::from_rgb_u8(46, 194, 126)) }
                                         else if n.signal > 75 { ("󰤨", Color::from_rgb_u8(46, 194, 126)) }
                                         else if n.signal > 50 { ("󰤥", Color::from_rgb_u8(245, 194, 17)) }
                                         else if n.signal > 25 { ("󰤢", Color::from_rgb_u8(255, 165, 0)) }
                                         else { ("󰤟", Color::from_rgb_u8(237, 51, 59)) };
                
                WifiNetworkItem {
                    ssid: n.ssid.into(),
                    signal: n.signal as i32,
                    security: n.security.into(),
                    connected: n.connected,
                    saved: n.saved,
                    ping: n.ping.map(|p| format!("{} ms", p)).unwrap_or_default().into(),
                    icon: icon.into(),
                    icon_color,
                    is_ethernet: n.is_ethernet,
                }
            }).collect();

            let vpns = nm_backend::list_vpns();
            let slint_vpns: Vec<WifiVpnItem> = vpns.into_iter().map(|v| {
                WifiVpnItem {
                    name: v.name.into(),
                    active: v.active,
                    type_name: v.vpn_type.into(),
                }
            }).collect();

            let ww = window_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ww.upgrade() {
                    ui.set_networks(ModelRc::new(VecModel::from(slint_networks)));
                    ui.set_vpns(ModelRc::new(VecModel::from(slint_vpns)));
                }
            });

            if fr_clone.swap(false, Ordering::SeqCst) {
                nm_backend::trigger_rescan();
            }

            for _ in 0..100 {
                if fr_clone.load(Ordering::SeqCst) { break; }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    });

    // Speed Monitoring
    let window_weak = main_window.as_weak();
    std::thread::spawn(move || {
        let mut last_stats: Option<(u64, u64, Instant)> = None;
        let mut download_history = VecDeque::from(vec![0.0; 60]);
        let mut upload_history = VecDeque::from(vec![0.0; 60]);
        let mut active_iface: Option<String> = None;
        let mut last_iface_check = Instant::now() - Duration::from_secs(10);

        loop {
            if last_iface_check.elapsed() > Duration::from_secs(5) {
                active_iface = nm_backend::get_active_interface();
                last_iface_check = Instant::now();
            }

            if let Some(ref iface) = active_iface {
                if let Some((rx, tx)) = nm_backend::get_interface_stats(iface) {
                    let now = Instant::now();
                    if let Some((l_rx, l_tx, l_time)) = last_stats {
                        let duration = now.duration_since(l_time).as_secs_f64();
                        if duration > 0.1 {
                            let rx_speed = (rx.wrapping_sub(l_rx) as f64) / duration;
                            let tx_speed = (tx.wrapping_sub(l_tx) as f64) / duration;
                            
                            download_history.pop_front();
                            download_history.push_back(rx_speed as f32);
                            upload_history.pop_front();
                            upload_history.push_back(tx_speed as f32);

                            let max = download_history.iter().chain(upload_history.iter())
                                .fold(1.0f32, |a, &b| a.max(b));

                            let dl_list: Vec<f32> = download_history.iter().cloned().collect();
                            let ul_list: Vec<f32> = upload_history.iter().cloned().collect();
                            
                            let dl_str = format_speed(rx_speed);
                            let ul_str = format_speed(tx_speed);

                            let ww = window_weak.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ww.upgrade() {
                                    ui.set_download_history(ModelRc::new(VecModel::from(dl_list)));
                                    ui.set_upload_history(ModelRc::new(VecModel::from(ul_list)));
                                    ui.set_max_speed(max);
                                    ui.set_current_download(dl_str.into());
                                    ui.set_current_upload(ul_str.into());
                                }
                            });
                            last_stats = Some((rx, tx, now));
                        }
                    } else {
                        last_stats = Some((rx, tx, now));
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    });

    // Pywal Live Monitoring
    let window_weak = main_window.as_weak();
    let config_clone = config.clone();
    std::thread::spawn(move || {
        let home = std::env::var("HOME").unwrap_or_default();
        let path = std::path::PathBuf::from(format!("{}/.cache/wal/colors.json", home));
        let mut last_mod = None;

        loop {
            let mut enabled = false;
            if let Ok(cfg) = config_clone.lock() {
                enabled = cfg.pywal_enabled;
            }

            if enabled {
                if let Ok(metadata) = std::fs::metadata(&path) {
                    if let Ok(mtime) = metadata.modified() {
                        if Some(mtime) != last_mod {
                            last_mod = Some(mtime);
                            apply_pywal_theme(window_weak.clone());
                        }
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    });

    main_window.run()
}
