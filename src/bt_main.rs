mod bt_backend;

use slint::{VecModel, Color, ModelRc, ComponentHandle};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
    } else {
        None
    }
}

fn apply_pywal_theme(handle: slint::Weak<AppWindow>) {
    let home = std::env::var("HOME").unwrap_or_default();
    let path = format!("{}/.cache/wal/colors.json", home);
    
    if let Ok(content) = std::fs::read_to_string(path) {
        if let Ok(wal) = serde_json::from_str::<PywalColors>(&content) {
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
}

// Intermediate struct that IS Send
struct InternalBluetoothDevice {
    name: String,
    address: String,
    connected: bool,
    paired: bool,
    trusted: bool,
    rssi: i32,
    audio_profiles: Vec<bt_backend::AudioProfile>,
}

fn main() -> Result<(), slint::PlatformError> {
    let main_window = AppWindow::new()?;
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
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ww.upgrade() {
                    ui.set_status_msg(if success { format!("Connected to {}", address) } else { "Failed!".to_string() }.into());
                }
            });
        });
    });

    let window_weak = main_window.as_weak();
    main_window.on_disconnect(move |address| {
        let address = address.to_string();
        let ww = window_weak.clone();
        std::thread::spawn(move || {
            let success = bt_backend::disconnect(&address);
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ww.upgrade() {
                    ui.set_status_msg(if success { format!("Disconnected {}", address) } else { "Failed!".to_string() }.into());
                }
            });
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
            }
        });
    });

    let window_weak = main_window.as_weak();
    main_window.on_toggle_pair(move |address, pair| {
        let address = address.to_string();
        let ww = window_weak.clone();
        std::thread::spawn(move || {
            let success = if pair { bt_backend::pair(&address) } else { true };
            if !success {
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ww.upgrade() {
                        ui.set_status_msg(format!("Pair failed for {}", address).into());
                    }
                });
            }
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

    main_window.on_refresh(|| { });

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

                        BluetoothDevice {
                            name: d.name.into(),
                            address: d.address.into(),
                            connected: d.connected,
                            paired: d.paired,
                            trusted: d.trusted,
                            rssi: d.rssi,
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

    main_window.run()
}
