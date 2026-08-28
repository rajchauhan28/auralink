// Shared with the `auralink-bt` binary; not every item is used by the `auralink` binary.
#![allow(dead_code)]

use std::process::{Child, Command};
use std::sync::Mutex;
use serde::{Deserialize, Serialize};

/// Run `bluetoothctl` with a hard session timeout, returning its combined
/// output.
///
/// Every call in this file used to be a bare `bluetoothctl <cmd>` read through
/// `.output()`. That BLOCKS FOREVER when the target device is out of range --
/// measured here: `bluetoothctl connect <unreachable-mac>` never returns,
/// while `bluetoothctl --timeout 5 connect <same>` exits after exactly 5s. In
/// the auto-connect daemon the unbounded form wedged the entire loop on the
/// first attempt at an absent device, which is the *normal* state for earbuds
/// sitting in their case.
///
/// Returns None only if bluetoothctl could not be spawned at all.
fn bluetoothctl(timeout_secs: u32, args: &[&str]) -> Option<String> {
    let timeout = timeout_secs.to_string();
    let mut full: Vec<&str> = vec!["--timeout", timeout.as_str()];
    full.extend_from_slice(args);

    Command::new("bluetoothctl")
        .args(&full)
        .output()
        .ok()
        .map(|o| {
            let mut text = String::from_utf8_lossy(&o.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&o.stderr));
            text
        })
}

/// Did a bluetoothctl operation actually succeed?
///
/// `bluetoothctl` exits 0 whether the operation worked or not -- it reports
/// the outcome only in its text -- so `o.status.success()`, which every caller
/// here relied on, was always true and every failure was reported as success.
fn bluetoothctl_ok(output: Option<String>, success_marker: &str) -> bool {
    match output {
        Some(text) => {
            let lower = text.to_lowercase();
            if lower.contains("failed")
                || lower.contains("not available")
                || lower.contains("org.bluez.error")
            {
                return false;
            }
            text.contains(success_marker)
        }
        None => false,
    }
}

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
pub struct Battery {
    /// Empty for a single/whole-device battery; otherwise a component name
    /// such as "Left", "Right" or "Case" derived from the D-Bus object path.
    pub label: String,
    pub percentage: i32,
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
    pub batteries: Vec<Battery>,
    pub audio_profiles: Vec<AudioProfile>,
}

/// Enumerate every `org.bluez.Battery1` object that belongs to a device.
///
/// Standard BlueZ exposes a single `Battery1` on the device path. Some TWS
/// earbuds / firmware expose one per component (left/right/case) on child
/// paths. We discover all of them via the object tree and read each
/// `Percentage`, so the UI renders however many actually exist (1, 2 or 3).
fn get_batteries(address: &str) -> Vec<Battery> {
    if !is_valid_mac(address) {
        return Vec::new();
    }
    let dev_id = format!("dev_{}", address.replace(':', "_"));
    let dev_path = format!("/org/bluez/hci0/{}", dev_id);

    // Direct property check for standard devices to avoid spawning expensive busctl tree commands
    let out = Command::new("busctl")
        .args([
            "--system",
            "get-property",
            "org.bluez",
            &dev_path,
            "org.bluez.Battery1",
            "Percentage",
        ])
        .output();

    let mut batteries = Vec::new();
    if let Ok(out) = out && out.status.success() {
        let stdout = String::from_utf8_lossy(&out.stdout);
        if let Some(num) = stdout.trim().strip_prefix("y ") {
            if let Ok(pct) = num.trim().parse::<i32>() {
                batteries.push(Battery { label: String::new(), percentage: pct });
                return batteries;
            }
        }
    }

    // Fall back to scanning object tree for multi-component devices (e.g. Left/Right/Case)
    let tree = match Command::new("busctl")
        .args(["--system", "tree", "org.bluez"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };
    let tree = String::from_utf8_lossy(&tree.stdout);
    let mut paths: Vec<String> = Vec::new();
    for line in tree.lines() {
        if let Some(idx) = line.find("/org/bluez/") {
            let path = line[idx..].trim().to_string();
            if path.contains(&dev_id) && !paths.contains(&path) {
                paths.push(path);
            }
        }
    }

    for path in &paths {
        let out = Command::new("busctl")
            .args([
                "--system",
                "get-property",
                "org.bluez",
                path,
                "org.bluez.Battery1",
                "Percentage",
            ])
            .output();
        let Ok(out) = out else { continue };
        if !out.status.success() {
            continue;
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        if let Some(num) = stdout.trim().strip_prefix("y ") {
            if let Ok(pct) = num.trim().parse::<i32>() {
                let label = battery_label(path, &dev_id);
                batteries.push(Battery { label, percentage: pct });
            }
        }
    }
    batteries
}

/// Derive a human-readable label from a battery object path. The whole-device
/// battery (path ends with the device id) gets an empty label; component
/// batteries are named from the trailing path segment.
fn battery_label(path: &str, dev_id: &str) -> String {
    let suffix = path.rsplit('/').next().unwrap_or("");
    if suffix == dev_id || suffix.is_empty() {
        return String::new();
    }
    let lower = suffix.to_lowercase();
    if lower.contains("left") {
        "Left".to_string()
    } else if lower.contains("right") {
        "Right".to_string()
    } else if lower.contains("case") {
        "Case".to_string()
    } else {
        // Title-case the raw segment as a best effort.
        let mut chars = suffix.chars();
        match chars.next() {
            Some(c) => c.to_uppercase().chain(chars).collect(),
            None => String::new(),
        }
    }
}

pub fn list_devices() -> Vec<BluetoothDevice> {
    // bluez-utils is an optional dependency, so bluetoothctl is not guaranteed
    // to exist. The previous .expect() turned "Bluetooth tooling not
    // installed" into a panic that took the whole application down; an empty
    // device list is the honest answer.
    let output = match Command::new("bluetoothctl").arg("devices").output() {
        Ok(output) => output,
        Err(_) => return Vec::new(),
    };

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
                    batteries: Vec::new(),
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
        batteries: Vec::new(),
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
        dev.batteries = get_batteries(address);
    }

    // Ensure `batteries` is always populated: fall back to the single value
    // parsed from `bluetoothctl info` when per-component data isn't exposed.
    if dev.batteries.is_empty() {
        if let Some(pct) = dev.battery {
            dev.batteries.push(Battery { label: String::new(), percentage: pct });
        }
    } else if dev.battery.is_none() {
        // Keep the legacy single field meaningful (lowest component).
        dev.battery = dev.batteries.iter().map(|b| b.percentage).min();
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
    // 20s: long enough for a paged device to answer, short enough that a UI
    // click does not appear to hang.
    bluetoothctl_ok(bluetoothctl(20, &["connect", address]), "Connection successful")
}

pub fn disconnect(address: &str) -> bool {
    bluetoothctl_ok(bluetoothctl(10, &["disconnect", address]), "Successful disconnected")
}

pub fn pair(address: &str) -> bool {
    // Pairing can involve user confirmation on the peer, so allow longer.
    bluetoothctl_ok(bluetoothctl(30, &["pair", address]), "Pairing successful")
}

pub fn trust(address: &str, enable: bool) -> bool {
    let cmd = if enable { "trust" } else { "untrust" };
    let marker = if enable { "trust succeeded" } else { "untrust succeeded" };
    bluetoothctl_ok(bluetoothctl(10, &[cmd, address]), marker)
}

pub fn remove(address: &str) -> bool {
    bluetoothctl_ok(bluetoothctl(10, &["remove", address]), "Device has been removed")
}

pub fn set_power(enable: bool) -> bool {
    let state = if enable { "on" } else { "off" };
    bluetoothctl_ok(bluetoothctl(10, &["power", state]), "succeeded")
}

pub fn toggle_power(enable: bool) -> bool {
    set_power(enable)
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

/// The `bluetoothctl ... scan on` child, if discovery is running.
///
/// Discovery needs a bluetoothctl session held open, so it has to be a
/// long-lived child. It is tracked here so it can be killed AND REAPED:
/// `start_scan` previously did `.spawn().is_ok()` and dropped the handle, so
/// nothing ever waited on the child. In the GUI that leaked a zombie now and
/// then; in the auto-connect daemon, which restarts discovery every time the
/// 60s session expires, it leaked one every minute for the life of the login
/// session.
static SCAN_CHILD: Mutex<Option<Child>> = Mutex::new(None);

/// How long a single discovery session runs before bluetoothctl exits.
pub const SCAN_SESSION_SECS: u64 = 60;

pub fn start_scan() -> bool {
    stop_scan();

    let _ = bluetoothctl(5, &["power", "on"]);

    match Command::new("bluetoothctl")
        .args(["--timeout", "60", "scan", "on"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(child) => {
            if let Ok(mut slot) = SCAN_CHILD.lock() {
                *slot = Some(child);
            }
            true
        }
        Err(_) => false,
    }
}

/// True while a discovery session this process started is still alive.
pub fn scan_running() -> bool {
    let Ok(mut slot) = SCAN_CHILD.lock() else { return false };
    match slot.as_mut() {
        // try_wait reaps it the moment it exits, so the 60s session ending
        // does not leave a zombie behind either.
        Some(child) => match child.try_wait() {
            Ok(Some(_)) => { *slot = None; false }
            Ok(None) => true,
            Err(_) => { *slot = None; false }
        },
        None => false,
    }
}

pub fn stop_scan() -> bool {
    // Kill and reap our own child rather than pkill'ing by pattern -- a
    // `pkill -f "bluetoothctl.*scan on"` also matches any scan the user
    // started by hand, and any process whose command line merely mentions it.
    if let Ok(mut slot) = SCAN_CHILD.lock()
        && let Some(mut child) = slot.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    
    // Also ask politely. Bounded like every other bluetoothctl call.
    bluetoothctl(5, &["scan", "off"]).is_some()
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

/// Every Bluetooth adapter BlueZ currently exposes, e.g. ["hci0"].
///
/// The device lookups below used to hardcode `hci0`. A USB dongle, a second
/// radio, or a controller that comes back as `hci1` after a suspend/resume
/// renumber left the auto-connect daemon looking at a path with no devices
/// under it -- so it found nothing to connect and did nothing, silently.
fn list_adapters() -> Vec<String> {
    let output = Command::new("dbus-send")
        .args(["--system", "--dest=org.bluez", "--print-reply",
               "/org/bluez", "org.freedesktop.DBus.Introspectable.Introspect"])
        .output()
        .ok();

    let mut adapters = Vec::new();
    if let Some(o) = output {
        let xml = String::from_utf8_lossy(&o.stdout);
        for node in xml.split("node name=\"").skip(1) {
            if let Some(end) = node.find('\"') {
                let name = &node[..end];
                if name.starts_with("hci") {
                    adapters.push(name.to_string());
                }
            }
        }
    }

    // Never return empty: a caller that gets no adapters would do nothing at
    // all, and hci0 is right on the overwhelming majority of machines.
    if adapters.is_empty() {
        adapters.push("hci0".to_string());
    }
    adapters
}

pub fn list_trusted() -> Vec<String> {
    let mut result = Vec::new();

    for adapter in list_adapters() {
        let output = Command::new("dbus-send")
            .args(["--system", "--dest=org.bluez", "--print-reply",
                   &format!("/org/bluez/{}", adapter),
                   "org.freedesktop.DBus.Introspectable.Introspect"])
            .output()
            .ok();

        let Some(o) = output else { continue };
        let xml = String::from_utf8_lossy(&o.stdout);

        for dev_tag in xml.split("node name=\"dev_").skip(1) {
            if let Some(end) = dev_tag.find('\"') {
                let addr_hex = &dev_tag[..end];
                let addr = addr_hex.replace("_", ":");

                if !is_valid_hex_addr(&addr) {
                    continue;
                }
                if result.contains(&addr) {
                    continue;
                }

                let out = Command::new("dbus-send")
                    .args(["--system", "--dest=org.bluez", "--print-reply",
                            &format!("/org/bluez/{}/dev_{}", adapter, addr.replace(':', "_")),
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

    // Re-assert trust cheaply; it is idempotent and survives a device reset
    // that cleared the bond.
    let _ = bluetoothctl(5, &["trust", address]);

    // 15s is the important number: unbounded, this call never returns for a
    // device that is switched off, and the daemon loop never runs again.
    bluetoothctl_ok(bluetoothctl(15, &["connect", address]), "Connection successful")
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
    // Switch a freshly connected headset to its best A2DP profile.
    //
    // The previous implementation scanned for the card's `Active Profile:` and
    // used it only if that line already contained "a2dp" -- so it could set
    // A2DP only when A2DP was *already* active, and was a no-op in exactly the
    // case it exists for: a device that came up on HSP/HFP. It now reads the
    // card's `Profiles:` block and picks the highest-priority available A2DP
    // sink, which is how PipeWire/PulseAudio advertise the real choices:
    //
    //   Profiles:
    //     a2dp-sink-sbc:    ... (priority: 132, available: yes)
    //     a2dp-sink:        ... codec AAC (priority: 133, available: yes)
    //     headset-head-unit: ... (priority: 6, available: yes)
    let card_suffix = address.replace(':', "_");

    let Some(output) = Command::new("pactl").args(["list", "cards"]).output().ok() else {
        return false;
    };
    let listing = String::from_utf8_lossy(&output.stdout);

    let mut card_name: Option<String> = None;
    let mut in_card = false;
    let mut in_profiles = false;
    let mut active_profile = String::new();
    let mut best: Option<(i64, String)> = None;

    for raw in listing.lines() {
        let line = raw.trim();

        if let Some(name) = line.strip_prefix("Name: ") {
            // A new card block starts here; only stay inside ours.
            in_card = name.contains(&card_suffix);
            in_profiles = false;
            if in_card {
                card_name = Some(name.to_string());
            }
            continue;
        }
        if !in_card {
            continue;
        }

        if line == "Profiles:" {
            in_profiles = true;
            continue;
        }
        if let Some(active) = line.strip_prefix("Active Profile: ") {
            active_profile = active.to_string();
            in_profiles = false;
            continue;
        }
        // Any other unindented section (Ports:, Properties:) ends the list.
        if in_profiles && line.ends_with(':') && !line.contains(' ') {
            in_profiles = false;
            continue;
        }

        if in_profiles && let Some((name, rest)) = line.split_once(": ") {
            if !name.starts_with("a2dp-sink") {
                continue;
            }
            if !rest.contains("available: yes") {
                continue;
            }
            let priority = rest
                .split("priority: ")
                .nth(1)
                .and_then(|s| s.split(|c: char| !c.is_ascii_digit()).next())
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0);
            if best.as_ref().is_none_or(|(p, _)| priority > *p) {
                best = Some((priority, name.to_string()));
            }
        }
    }

    let (Some(card), Some((_, profile))) = (card_name, best) else {
        return false;
    };
    if active_profile == profile {
        return true;
    }

    Command::new("pactl")
        .args(["set-card-profile", &card, &profile])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
