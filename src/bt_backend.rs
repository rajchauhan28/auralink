use std::process::Command;
use serde::{Deserialize, Serialize};

fn is_valid_mac(addr: &str) -> bool {
    let parts: Vec<&str> = addr.split(':').collect();
    if parts.len() != 6 {
        return false;
    }
    parts.iter().all(|p| {
        p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit())
    })
}

fn is_valid_hex_addr(addr: &str) -> bool {
    let addr = addr.replace('_', ":");
    is_valid_mac(&addr)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioProfile {
    pub name: String,
    pub description: String,
    pub active: bool,
    pub available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BluetoothDevice {
    pub name: String,
    pub address: String,
    pub connected: bool,
    pub paired: bool,
    pub trusted: bool,
    pub rssi: i32,
    pub battery: Option<i32>,
    pub audio_profiles: Vec<AudioProfile>,
}

pub fn list_devices() -> Vec<BluetoothDevice> {
    let output = Command::new("bluetoothctl")
        .arg("devices")
        .output()
        .expect("failed to execute bluetoothctl");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut devices = Vec::new();

    for line in stdout.lines() {
        // Device 00:00:00:00:00:00 Name
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 && parts[0] == "Device" {
            let address = parts[1].to_string();
            let name = parts[2..].join(" ");
            
            // Get detailed info for each device
            if let Some(info) = get_device_info(&address) {
                let mut dev = info;
                if dev.name.is_empty() { dev.name = name; }
                devices.push(dev);
            } else {
                devices.push(BluetoothDevice {
                    name,
                    address,
                    connected: false,
                    paired: false,
                    trusted: false,
                    rssi: 0,
                    battery: None,
                    audio_profiles: Vec::new(),
                });
            }
        }
    }
    
    devices.sort_by(|a, b| {
        b.connected.cmp(&a.connected)
            .then_with(|| b.paired.cmp(&a.paired))
            .then_with(|| a.name.cmp(&b.name))
    });
    
    devices
}

pub fn get_device_info(address: &str) -> Option<BluetoothDevice> {
    let output = Command::new("bluetoothctl")
        .args(["info", address])
        .output()
        .ok()?;
    
    if !output.status.success() { return None; }
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut dev = BluetoothDevice {
        name: String::new(),
        address: address.to_string(),
        connected: false,
        paired: false,
        trusted: false,
        rssi: 0,
        battery: None,
        audio_profiles: Vec::new(),
    };

    for line in stdout.lines() {
        let line = line.trim();
        if line.starts_with("Name:") {
            dev.name = line[5..].trim().to_string();
        } else if line.starts_with("Connected:") {
            dev.connected = line[10..].trim() == "yes";
        } else if line.starts_with("Paired:") {
            dev.paired = line[7..].trim() == "yes";
        } else if line.starts_with("Trusted:") {
            dev.trusted = line[8..].trim() == "yes";
        } else if line.starts_with("RSSI:") {
            dev.rssi = line[5..].trim().parse().unwrap_or(0);
        } else if line.contains("Battery Percentage:") {
            if let Some(start) = line.find('(') {
                if let Some(end) = line.find(')') {
                    dev.battery = line[start+1..end].parse().ok();
                }
            } else {
                // Try parsing direct number if no parens
                let parts: Vec<&str> = line.split_whitespace().collect();
                if let Some(last) = parts.last() {
                    dev.battery = last.trim_matches('%').parse().ok();
                }
            }
        }
    }

    if dev.connected {
        dev.audio_profiles = get_audio_profiles(address);
    }
    
    Some(dev)
}

fn get_audio_profiles(address: &str) -> Vec<AudioProfile> {
    let output = Command::new("pactl")
        .args(["list", "cards"])
        .output()
        .ok();
    
    let mut profiles = Vec::new();
    if let Some(o) = output {
        let stdout = String::from_utf8_lossy(&o.stdout);
        let parts: Vec<&str> = stdout.split("Card #").collect();
        
        // Find the card for this bluetooth address
        let addr_formatted = address.replace(':', "_");
        for card in parts {
            if card.contains(&addr_formatted) || card.contains(address) {
                // Found the card, now parse profiles
                let mut in_profiles = false;
                let mut active_profile = String::new();
                
                for line in card.lines() {
                    let line = line.trim();
                    if line.starts_with("Active Profile:") {
                        active_profile = line[15..].trim().to_string();
                    } else if line.starts_with("Profiles:") {
                        in_profiles = true;
                    } else if line.starts_with("Ports:") {
                        in_profiles = false;
                    } else if in_profiles {
                        // Example: a2dp-sink: High Fidelity Playback (A2DP Sink, codec AAC) (sinks: 1, sources: 0, priority: 133, available: yes)
                        if let Some(colon_idx) = line.find(':') {
                            let name = line[..colon_idx].trim().to_string();
                            let rest = &line[colon_idx+1..];
                            
                            let (desc, available) = if let Some(paren_idx) = rest.rfind('(') {
                                let desc_text = rest[..paren_idx].trim().to_string();
                                let avail_part = &rest[paren_idx..];
                                let avail = avail_part.contains("available: yes");
                                (desc_text, avail)
                            } else {
                                (rest.trim().to_string(), true)
                            };
                            
                            profiles.push(AudioProfile {
                                name: name.clone(),
                                description: desc,
                                active: name == active_profile,
                                available,
                            });
                        }
                    }
                }
                break;
            }
        }
    }
    profiles
}

pub fn set_audio_profile(address: &str, profile_name: &str) -> bool {
    // Find the card name first
    let output = Command::new("pactl")
        .args(["list", "cards", "short"])
        .output()
        .ok();
    
    let mut card_name = None;
    if let Some(o) = output {
        let stdout = String::from_utf8_lossy(&o.stdout);
        let addr_formatted = address.replace(':', "_");
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 && (parts[1].contains(&addr_formatted) || parts[1].contains(address)) {
                card_name = Some(parts[1].to_string());
                break;
            }
        }
    }

    if let Some(card) = card_name {
        Command::new("pactl")
            .args(["set-card-profile", &card, profile_name])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    } else {
        false
    }
}

pub fn connect(address: &str) -> bool {
    Command::new("bluetoothctl")
        .args(["connect", address])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn disconnect(address: &str) -> bool {
    Command::new("bluetoothctl")
        .args(["disconnect", address])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn pair(address: &str) -> bool {
    Command::new("bluetoothctl")
        .args(["pair", address])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn trust(address: &str, enable: bool) -> bool {
    let cmd = if enable { "trust" } else { "untrust" };
    Command::new("bluetoothctl")
        .args([cmd, address])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn remove(address: &str) -> bool {
    Command::new("bluetoothctl")
        .args(["remove", address])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn set_power(enable: bool) -> bool {
    let state = if enable { "on" } else { "off" };
    Command::new("bluetoothctl")
        .args(["power", state])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn start_scan() -> bool {
    stop_scan();

    let _ = Command::new("bluetoothctl")
        .args(["power", "on"])
        .output();

    Command::new("bluetoothctl")
        .args(["--timeout", "60", "scan", "on"])
        .spawn()
        .is_ok()
}

pub fn stop_scan() -> bool {
    // Kill any existing bluetoothctl scan processes started by start_scan
    // This is necessary because bluetoothctl scan on must be kept open to maintain discovery.
    let _ = Command::new("pkill")
        .args(["-f", "bluetoothctl.*scan on"])
        .output();
    
    // Also try the official way
    Command::new("bluetoothctl")
        .args(["scan", "off"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn list_connected_devices() -> Vec<BluetoothDevice> {
    let output = Command::new("bluetoothctl")
        .args(["devices", "Connected"])
        .output()
        .ok();
        
    let mut devices = Vec::new();
    if let Some(o) = output {
        let stdout = String::from_utf8_lossy(&o.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 && parts[0] == "Device" {
                let address = parts[1];
                if let Some(info) = get_device_info(address) {
                    devices.push(info);
                }
            }
        }
    }
    devices
}

pub fn is_powered() -> bool {
    let output = Command::new("bluetoothctl")
        .args(["show"])
        .output()
        .ok();
        
    if let Some(o) = output {
        let stdout = String::from_utf8_lossy(&o.stdout);
        return stdout.contains("Powered: yes");
    }
    false
}

pub fn list_trusted() -> Vec<String> {
    let output = Command::new("dbus-send")
        .args(["--system", "--dest=org.bluez", "--print-reply", 
                "/org/bluez/hci0", "org.freedesktop.DBus.Introspectable.Introspect"])
        .output()
        .ok();

    let mut result = Vec::new();
    if let Some(o) = output {
        let xml = String::from_utf8_lossy(&o.stdout);
        
        for dev_tag in xml.split("node name=\"dev_").skip(1) {
            if let Some(end) = dev_tag.find('\"') {
                let addr_hex = &dev_tag[..end];
                let addr = addr_hex.replace("_", ":");
                
                if !is_valid_hex_addr(&addr) {
                    continue;
                }

                let out = Command::new("dbus-send")
                    .args(["--system", "--dest=org.bluez", "--print-reply",
                            &format!("/org/bluez/hci0/dev_{}", addr.replace(':', "_")),
                            "org.freedesktop.DBus.Properties.Get",
                            "string:org.bluez.Device1", "string:Trusted"])
                    .output()
                    .ok();

                if let Some(o) = out {
                    let output_str = String::from_utf8_lossy(&o.stdout);
                    if output_str.contains("boolean true") {
                        result.push(addr);
                    }
                }
            }
        }
    }

    result
}

pub fn get_connected_address() -> Option<String> {
    let output = Command::new("bluetoothctl")
        .args(["devices", "Connected"])
        .output()
        .ok();

    if let Some(o) = output {
        let stdout = String::from_utf8_lossy(&o.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 && parts[0] == "Device" {
                let addr = parts[1];
                if is_valid_mac(addr) {
                    return Some(addr.to_string());
                }
            }
        }
    }
    None
}

pub fn connect_trusted_device(address: &str) -> bool {
    if let Some(connected) = get_connected_address()
        && connected == address {
            return true;
        }

    let _ = Command::new("bluetoothctl")
        .args(["trust", address])
        .output();

    Command::new("bluetoothctl")
        .args(["connect", address])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn ensure_switch_on_connect_module() -> bool {
    let output = Command::new("pactl")
        .args(["list", "modules"])
        .output()
        .ok();

    if let Some(o) = output {
        let stdout = String::from_utf8_lossy(&o.stdout);
        if stdout.contains("module-switch-on-connect") {
            return true;
        }
    }

    Command::new("pactl")
        .args(["load-module", "module-switch-on-connect"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn force_a2dp_profile(address: &str) -> bool {
    let card_formatted = address.replace(':', "_");
    let output = Command::new("pactl")
        .args(["list", "cards", "short"])
        .output()
        .ok();

    if let Some(o) = output {
        let stdout = String::from_utf8_lossy(&o.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 && parts[1].contains(&card_formatted) {
                let card_name = &parts[1];
                
                let profile_output = Command::new("pactl")
                    .args(["list", "cards"])
                    .output()
                    .ok();

                if let Some(po) = profile_output {
                    let pstdout = String::from_utf8_lossy(&po.stdout);
                    let mut in_card = false;
                    let mut a2dp_profile = String::new();

                    for line in pstdout.lines() {
                        let line = line.trim();
                        if line.starts_with("Name:") && line.contains(&card_formatted) {
                            in_card = true;
                        } else if line.starts_with("Name:") && !line.contains(&card_formatted) {
                            in_card = false;
                        }

                        if in_card && line.starts_with("Active Profile:") {
                            let profile = line[15..].trim();
                            if profile.contains("a2dp") && !profile.contains("handsfree") && !profile.contains("headset") {
                                a2dp_profile = profile.to_string();
                                break;
                            }
                        }
                    }

                    if !a2dp_profile.is_empty() {
                        return Command::new("pactl")
                            .args(["set-card-profile", card_name, &a2dp_profile])
                            .output()
                            .map(|o| o.status.success())
                            .unwrap_or(false);
                    }
                }
                break;
            }
        }
    }

    false
}
