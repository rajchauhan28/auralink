use std::process::Command;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Network {
    pub ssid: String,
    pub bssid: String,
    pub signal: u8,
    pub security: String,
    pub connected: bool,
    pub ping: Option<u64>,
    pub saved: bool,
}

pub fn list_networks() -> Vec<Network> {
    let saved_ssids = get_saved_ssids();
    let active_ssid = get_active_ssid();

    let output = Command::new("nmcli")
        .args(&["-t", "-f", "SSID,BSSID,SIGNAL,SECURITY,IN-USE", "dev", "wifi", "list"])
        .output()
        .expect("failed to execute nmcli");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut networks = Vec::new();
    let mut seen_ssids = HashSet::new();

    for line in stdout.lines() {
        let mut parts = Vec::new();
        let mut current = String::new();
        let mut escaped = false;
        
        for c in line.chars() {
            if escaped {
                current.push(c);
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == ':' {
                parts.push(current.clone());
                current.clear();
            } else {
                current.push(c);
            }
        }
        parts.push(current);

        if parts.len() >= 5 {
            let ssid = parts[0].to_string();
            if ssid.is_empty() || seen_ssids.contains(&ssid) { continue; }
            
            let is_connected = parts[4] == "*" || active_ssid.as_deref() == Some(&ssid);
            let is_saved = saved_ssids.contains(&ssid);
            
            networks.push(Network {
                ssid: ssid.clone(),
                bssid: parts[1].to_string(),
                signal: parts[2].parse().unwrap_or(0),
                security: parts[3].to_string(),
                connected: is_connected,
                ping: None,
                saved: is_saved,
            });
            seen_ssids.insert(ssid);
        }
    }
    
    networks.sort_by(|a, b| {
        b.connected.cmp(&a.connected)
            .then_with(|| b.saved.cmp(&a.saved))
            .then_with(|| b.signal.cmp(&a.signal))
    });
    
    networks
}

fn get_active_ssid() -> Option<String> {
    let output = Command::new("nmcli")
        .args(&["-t", "-f", "ACTIVE,SSID", "dev", "wifi"])
        .output()
        .ok()?;
    
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find(|l| l.starts_with("yes:"))
        .map(|l| {
            let mut ssid = String::new();
            let mut escaped = false;
            let content = &l[4..]; 
            for c in content.chars() {
                if escaped { ssid.push(c); escaped = false; }
                else if c == '\\' { escaped = true; }
                else { ssid.push(c); }
            }
            ssid
        })
}

fn get_saved_ssids() -> HashSet<String> {
    let output = Command::new("nmcli")
        .args(&["-t", "-f", "NAME,TYPE", "connection", "show"])
        .output()
        .ok();

    let mut saved = HashSet::new();
    if let Some(o) = output {
        let stdout = String::from_utf8_lossy(&o.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 2 && parts[1].contains("wireless") {
                saved.insert(parts[0].to_string());
            }
        }
    }
    saved
}

pub fn trigger_rescan() {
    let _ = Command::new("nmcli").args(&["dev", "wifi", "rescan"]).spawn();
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpnConnection {
    pub name: String,
    pub vpn_type: String,
    pub active: bool,
}

pub fn list_vpns() -> Vec<VpnConnection> {
    let mut vpns = Vec::new();

    if let Ok(output) = Command::new("nmcli").args(&["-t", "-f", "NAME,TYPE,STATE", "c", "show"]).output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 3 {
                let name = parts[0];
                let ctype = parts[1];
                let state = parts[2];
                if ctype == "wireguard" || ctype == "vpn" || ctype == "openvpn" {
                    vpns.push(VpnConnection {
                        name: name.to_string(),
                        vpn_type: ctype.to_string(),
                        active: state == "activated",
                    });
                }
            }
        }
    }

    let warp_active = Command::new("systemctl").args(&["is-active", "warp-svc"]).output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "active")
        .unwrap_or(false);

    if warp_active {
        if let Ok(output) = Command::new("warp-cli").arg("status").output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            vpns.push(VpnConnection {
                name: "Cloudflare WARP".to_string(),
                vpn_type: "warp".to_string(),
                active: stdout.contains("Status update: Connected") || stdout.contains("Connected"),
            });
        }
    }

    if let Ok(output) = Command::new("ip").args(&["link", "show", "hiddify-tun"]).output() {
        if output.status.success() {
             vpns.push(VpnConnection {
                name: "Hiddify".to_string(),
                vpn_type: "hiddify".to_string(),
                active: true,
            });
        } else {
             if let Ok(output) = Command::new("pgrep").arg("-x").arg("hiddify").output() {
                 if output.status.success() {
                     vpns.push(VpnConnection {
                        name: "Hiddify".to_string(),
                        vpn_type: "hiddify".to_string(),
                        active: true,
                    });
                 }
             }
        }
    }

    vpns
}

pub fn toggle_vpn(name: &str, vpn_type: &str, enable: bool) -> bool {
    if vpn_type == "warp" {
        let arg = if enable { "connect" } else { "disconnect" };
        Command::new("warp-cli").arg(arg).output().map(|o| o.status.success()).unwrap_or(false)
    } else if vpn_type == "hiddify" {
        false
    } else {
        let arg = if enable { "up" } else { "down" };
        Command::new("nmcli").args(&["c", arg, name]).output().map(|o| o.status.success()).unwrap_or(false)
    }
}

pub fn import_vpn(file_path: &str) -> bool {
    let p = std::path::Path::new(file_path);
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
    let vpn_type = match ext {
        "conf" => "wireguard",
        "ovpn" => "openvpn",
        _ => return false,
    };
    
    Command::new("nmcli")
        .args(&["connection", "import", "type", vpn_type, "file", file_path])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn forget_network(ssid: &str) -> bool {
    let _ = Command::new("nmcli")
        .args(&["connection", "down", "id", ssid])
        .output();
    Command::new("nmcli")
        .args(&["connection", "delete", "id", ssid])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn get_network_info(ssid: &str) -> String {
    let output = Command::new("nmcli")
        .args(&["-t", "-f", "GENERAL,IP4,IP6,PROXY", "connection", "show", ssid])
        .output()
        .ok();
        
    if let Some(o) = output {
        if o.status.success() {
            return String::from_utf8_lossy(&o.stdout).to_string();
        }
    }
    
    format!("SSID: {}\nNo detailed connection info available.", ssid)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub autoconnect: bool,
    pub priority: i32,
    pub dns: String,
    pub ip4_method: String,
}

pub fn get_network_config(ssid: &str) -> Option<NetworkConfig> {
    let output = Command::new("nmcli")
        .args(&["-t", "-f", "connection.autoconnect,connection.autoconnect-priority,ipv4.dns,ipv4.method", "connection", "show", ssid])
        .output()
        .ok()?;
        
    if !output.status.success() { return None; }
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut config = NetworkConfig {
        autoconnect: true,
        priority: 0,
        dns: String::new(),
        ip4_method: "auto".to_string(),
    };
    
    for line in stdout.lines() {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() < 2 { continue; }
        match parts[0] {
            "connection.autoconnect" => config.autoconnect = parts[1] == "yes",
            "connection.autoconnect-priority" => config.priority = parts[1].parse().unwrap_or(0),
            "ipv4.dns" => config.dns = parts[1].to_string(),
            "ipv4.method" => config.ip4_method = parts[1].to_string(),
            _ => {}
        }
    }
    Some(config)
}

pub fn update_network_config(ssid: &str, config: NetworkConfig) -> bool {
    let mut cmd = Command::new("nmcli");
    cmd.args(&["connection", "modify", ssid,
        "connection.autoconnect", if config.autoconnect { "yes" } else { "no" },
        "connection.autoconnect-priority", &config.priority.to_string(),
        "ipv4.method", &config.ip4_method]);
    
    if !config.dns.is_empty() {
        cmd.args(&["ipv4.dns", &config.dns]);
    }

    cmd.output().map(|o| o.status.success()).unwrap_or(false)
}

pub fn get_ping(host: &str) -> Option<u64> {
    let output = Command::new("ping")
        .args(&["-c", "1", "-W", "1", host])
        .output()
        .ok()?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(time_part) = stdout.split("time=").nth(1) {
            if let Some(ms_str) = time_part.split_whitespace().next() {
                return ms_str.parse::<f64>().ok().map(|f| f as u64);
            }
        }
    }
    None
}

pub fn get_active_interface() -> Option<String> {
    let output = Command::new("nmcli")
        .args(&["-t", "-f", "DEVICE,TYPE,STATE", "dev"])
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() >= 3 && parts[1] == "wifi" && parts[2] == "connected" {
            return Some(parts[0].to_string());
        }
    }
    None
}

pub fn get_interface_stats(iface: &str) -> Option<(u64, u64)> {
    let content = std::fs::read_to_string("/proc/net/dev").ok()?;
    for line in content.lines() {
        if line.contains(iface) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 10 {
                let rx = parts[1].parse::<u64>().unwrap_or(0);
                let tx = parts[9].parse::<u64>().unwrap_or(0);
                return Some((rx, tx));
            }
        }
    }
    None
}

pub fn connect_to_wifi(ssid: &str, password: Option<&str>) -> bool {
    if let Some(pass) = password {
        let mut cmd = Command::new("nmcli");
        cmd.args(&["--wait", "15", "dev", "wifi", "connect", ssid, "password", pass]);
        cmd.output().map(|o| o.status.success()).unwrap_or(false)
    } else {
        let mut cmd = Command::new("nmcli");
        cmd.args(&["--wait", "15", "connection", "up", "id", ssid]);
        let output = cmd.output();
        
        if let Ok(o) = output {
            if o.status.success() {
                return true;
            }
        }
        
        let mut cmd2 = Command::new("nmcli");
        cmd2.args(&["--wait", "15", "dev", "wifi", "connect", ssid]);
        cmd2.output().map(|o| o.status.success()).unwrap_or(false)
    }
}

pub fn disconnect_wifi() -> bool {
    let output = Command::new("nmcli")
        .args(&["-t", "-f", "DEVICE,TYPE,STATE", "dev"])
        .output()
        .ok();

    if let Some(o) = output {
        let stdout = String::from_utf8_lossy(&o.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 3 && parts[1] == "wifi" && parts[2] == "connected" {
                let dev = parts[0];
                let _ = Command::new("nmcli").args(&["dev", "disconnect", dev]).output();
                return true;
            }
        }
    }
    false
}
