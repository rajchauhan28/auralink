use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Network {
    pub ssid: String,
    pub bssid: String,
    pub signal: u8,
    pub security: String,
    pub connected: bool,
    pub ping: Option<u64>,
    pub saved: bool,
    pub is_ethernet: bool,
}

fn parse_nmcli_line(line: &str) -> Vec<String> {
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
    parts
}

/// One row of `nmcli connection show`.
struct Connection {
    name: String,
    ctype: String,
    /// "activated", "activating", ... or empty when inactive.
    state: String,
    device: String,
}

/// Every saved connection, in one nmcli call. Feeds the saved-network flags,
/// the wired entries and the VPN list, which used to make three calls.
fn connections() -> Vec<Connection> {
    let Ok(output) = Command::new("nmcli")
        .args(["-t", "-f", "NAME,TYPE,STATE,DEVICE", "connection", "show"])
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(parse_nmcli_line)
        .filter(|p| p.len() >= 4)
        .map(|p| Connection {
            name: p[0].clone(),
            ctype: p[1].clone(),
            state: p[2].clone(),
            device: p[3].clone(),
        })
        .collect()
}

fn is_ethernet(ctype: &str) -> bool {
    ctype == "802-3-ethernet" || ctype == "ethernet"
}

/// Wired connections that are up, then visible Wi-Fi networks, best first.
///
/// `allow_rescan` = false reads NetworkManager's cached scan results.
/// nmcli's default (`--rescan auto`) starts a new scan whenever the cache is
/// older than 30s *and blocks until it finishes*; polled every 10s by the
/// window and every few seconds by Waybar via `status`, that kept the radio
/// scanning around the clock, which costs latency spikes on the live link.
/// The window now requests scans itself (see trigger_rescan).
pub fn list_networks(allow_rescan: bool) -> Vec<Network> {
    networks_from(&connections(), allow_rescan)
}

fn networks_from(conns: &[Connection], allow_rescan: bool) -> Vec<Network> {
    let rescan = if allow_rescan { "auto" } else { "no" };
    let rows: Vec<Vec<String>> = Command::new("nmcli")
        .args([
            "-t",
            "-f",
            "SSID,BSSID,SIGNAL,SECURITY,IN-USE",
            "device",
            "wifi",
            "list",
            "--rescan",
            rescan,
        ])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(parse_nmcli_line)
                .filter(|p| p.len() >= 5)
                .collect()
        })
        .unwrap_or_default();
    build_networks(conns, &rows)
}

/// `rows` are parsed `nmcli -t -f SSID,BSSID,SIGNAL,SECURITY,IN-USE` lines.
fn build_networks(conns: &[Connection], rows: &[Vec<String>]) -> Vec<Network> {
    let mut networks: Vec<Network> = conns
        .iter()
        .filter(|c| is_ethernet(&c.ctype) && c.state == "activated")
        .map(|c| Network {
            ssid: c.name.clone(),
            bssid: c.device.clone(), // device name stands in for a BSSID
            signal: 100,
            security: "Wired".to_string(),
            connected: true,
            ping: None,
            saved: true,
            is_ethernet: true,
        })
        .collect();

    let saved: HashSet<&str> = conns
        .iter()
        .filter(|c| c.ctype.contains("wireless") || is_ethernet(&c.ctype))
        .map(|c| c.name.as_str())
        .collect();

    // An SSID is in use if ANY of its access points is.
    let active: HashSet<&str> = rows
        .iter()
        .filter(|p| p[4] == "*")
        .map(|p| p[0].as_str())
        .collect();

    // One entry per SSID, keeping its strongest access point.
    let mut index: HashMap<String, usize> = HashMap::new();
    for p in rows {
        let ssid = &p[0];
        if ssid.is_empty() {
            continue; // hidden network
        }
        let signal: u8 = p[2].parse().unwrap_or(0);
        if let Some(&i) = index.get(ssid) {
            if signal > networks[i].signal {
                networks[i].signal = signal;
                networks[i].bssid = p[1].clone();
            }
            continue;
        }
        index.insert(ssid.clone(), networks.len());
        networks.push(Network {
            ssid: ssid.clone(),
            bssid: p[1].clone(),
            signal,
            security: p[3].clone(),
            connected: active.contains(ssid.as_str()),
            ping: None,
            saved: saved.contains(ssid.as_str()),
            is_ethernet: false,
        });
    }

    networks.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then_with(|| b.is_ethernet.cmp(&a.is_ethernet))
            .then_with(|| b.saved.cmp(&a.saved))
            .then_with(|| b.signal.cmp(&a.signal))
    });
    networks
}

/// Ask NetworkManager for a fresh Wi-Fi scan. Returns once the request is
/// accepted; results land in the cache a few seconds later.
pub fn trigger_rescan() {
    // status(), not spawn(): a dropped Child is never reaped and stays a
    // zombie for the life of the window.
    let _ = Command::new("nmcli")
        .args(["device", "wifi", "rescan"])
        .status();
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpnConnection {
    pub name: String,
    pub vpn_type: String,
    pub active: bool,
}

/// Networks and VPNs from a single `nmcli connection show`.
pub fn list_networks_and_vpns(allow_rescan: bool) -> (Vec<Network>, Vec<VpnConnection>) {
    let conns = connections();
    (networks_from(&conns, allow_rescan), vpns_from(&conns))
}

fn vpns_from(conns: &[Connection]) -> Vec<VpnConnection> {
    let mut vpns: Vec<VpnConnection> = conns
        .iter()
        .filter(|c| matches!(c.ctype.as_str(), "wireguard" | "vpn" | "openvpn"))
        .map(|c| VpnConnection {
            name: c.name.clone(),
            vpn_type: c.ctype.clone(),
            active: c.state == "activated",
        })
        .collect();

    let warp_service_up = Command::new("systemctl")
        .args(["is-active", "--quiet", "warp-svc"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if warp_service_up && let Ok(output) = Command::new("warp-cli").arg("status").output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        vpns.push(VpnConnection {
            name: "Cloudflare WARP".to_string(),
            vpn_type: "warp".to_string(),
            // Case-sensitive on purpose: "Disconnected" does not match.
            active: stdout.contains("Connected"),
        });
    }

    // Hiddify: its tun device, else its process. A sysfs check instead of
    // spawning `ip link show` on every poll.
    let hiddify_up = std::path::Path::new("/sys/class/net/hiddify-tun").exists()
        || Command::new("pgrep")
            .args(["-x", "hiddify"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
    if hiddify_up {
        vpns.push(VpnConnection {
            name: "Hiddify".to_string(),
            vpn_type: "hiddify".to_string(),
            active: true,
        });
    }

    vpns
}

pub fn toggle_vpn(name: &str, vpn_type: &str, enable: bool) -> bool {
    if vpn_type == "warp" {
        let arg = if enable { "connect" } else { "disconnect" };
        Command::new("warp-cli")
            .arg(arg)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    } else if vpn_type == "hiddify" {
        let cmd = if enable { "start" } else { "stop" };
        Command::new("systemctl")
            .args([cmd, "hiddify-agent"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    } else {
        let arg = if enable { "up" } else { "down" };
        Command::new("nmcli")
            .args(["c", arg, name])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
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
        .args(["connection", "import", "type", vpn_type, "file", file_path])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn forget_network(ssid: &str) -> bool {
    let _ = Command::new("nmcli")
        .args(["connection", "down", "id", ssid])
        .output();
    Command::new("nmcli")
        .args(["connection", "delete", "id", ssid])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn get_network_info(ssid: &str) -> String {
    let output = Command::new("nmcli")
        .args(["connection", "show", ssid])
        .output()
        .ok();

    if let Some(o) = output
        && o.status.success()
    {
        let full_info = String::from_utf8_lossy(&o.stdout).to_string();
        let mut result = Vec::new();

        for line in full_info.lines() {
            if (line.contains("GENERAL.")
                || line.contains("IP4.")
                || line.contains("IP6.")
                || line.contains("DHCP4.")
                || line.contains("802-11-wireless"))
                && let Some(sep_idx) = line.find(':')
            {
                let key = line[..sep_idx].trim();
                let value = line[sep_idx + 1..].trim();

                // Make key more readable: "IP4.ADDRESS[1]" -> "IP4 ADDRESS"
                let clean_key = key
                    .replace(['.', '['], " ")
                    .replace(']', "")
                    .replace('-', " ");

                if !value.is_empty() && value != "--" {
                    result.push(format!("{}: {}", clean_key.to_uppercase(), value));
                }
            }
        }

        if result.is_empty() {
            return "No active connection details.".to_string();
        }
        return result.join("\n");
    }

    format!("SSID: {}\nNo detailed connection info available.", ssid)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NetworkConfig {
    pub autoconnect: bool,
    pub priority: i32,
    pub dns: String,
    pub ipv4_method: String,
    pub ipv4_address: String,
    pub ipv4_gateway: String,
    pub ipv6_method: String,
    pub ipv6_address: String,
    pub ipv6_gateway: String,
    pub mac_address: String,
    pub password: String,
}

pub fn get_network_config(ssid: &str) -> Option<NetworkConfig> {
    let fields = [
        "connection.type",
        "connection.autoconnect",
        "connection.autoconnect-priority",
        "ipv4.dns",
        "ipv4.method",
        "ipv4.addresses",
        "ipv4.gateway",
        "ipv6.method",
        "ipv6.addresses",
        "ipv6.gateway",
        "802-11-wireless.cloned-mac-address",
        "802-11-wireless-security.psk",
        "802-3-ethernet.cloned-mac-address",
    ];

    let output = Command::new("nmcli")
        .args([
            "-s",
            "-t",
            "-f",
            &fields.join(","),
            "connection",
            "show",
            ssid,
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut config = NetworkConfig {
        autoconnect: true,
        priority: 0,
        dns: String::new(),
        ipv4_method: "auto".to_string(),
        ipv4_address: String::new(),
        ipv4_gateway: String::new(),
        ipv6_method: "auto".to_string(),
        ipv6_address: String::new(),
        ipv6_gateway: String::new(),
        mac_address: String::new(),
        password: String::new(),
    };

    // First pass: determine connection type from the nmcli output
    let mut is_eth = false;
    for line in stdout.lines() {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() >= 2 && parts[0] == "connection.type" {
            is_eth = parts[1].contains("ethernet");
            break;
        }
    }

    // Second pass: populate config fields
    for line in stdout.lines() {
        let parts = parse_nmcli_line(line);
        if parts.len() < 2 {
            continue;
        }
        match parts[0].as_str() {
            "connection.autoconnect" => config.autoconnect = parts[1] == "yes",
            "connection.autoconnect-priority" => config.priority = parts[1].parse().unwrap_or(0),
            "ipv4.dns" => config.dns = parts[1].to_string(),
            "ipv4.method" => config.ipv4_method = parts[1].to_string(),
            "ipv4.addresses" => config.ipv4_address = parts[1].to_string(),
            "ipv4.gateway" => config.ipv4_gateway = parts[1].to_string(),
            "ipv6.method" => config.ipv6_method = parts[1].to_string(),
            "ipv6.addresses" => config.ipv6_address = parts[1].to_string(),
            "ipv6.gateway" => config.ipv6_gateway = parts[1].to_string(),
            "802-11-wireless.cloned-mac-address" => {
                if !is_eth {
                    config.mac_address = parts[1].to_string()
                }
            }
            "802-3-ethernet.cloned-mac-address" => {
                if is_eth {
                    config.mac_address = parts[1].to_string()
                }
            }
            "802-11-wireless-security.psk" => config.password = parts[1].to_string(),
            _ => {}
        }
    }
    Some(config)
}

/// nmcli's error text for a failed run, or a generic one.
fn nmcli_error(out: &std::process::Output) -> String {
    let err = String::from_utf8_lossy(&out.stderr);
    let err = err.trim();
    if err.is_empty() {
        "nmcli failed".to_string()
    } else {
        err.trim_start_matches("Error: ").to_string()
    }
}

/// Save the Advanced dialog. Everything except the password goes through
/// `nmcli connection modify`; the password goes over D-Bus (see nm_dbus) so
/// it never appears in a process argument list.
pub fn update_network_config(ssid: &str, config: NetworkConfig) -> Result<(), String> {
    let mut args = vec![
        "connection".to_string(),
        "modify".to_string(),
        ssid.to_string(),
        "connection.autoconnect".to_string(),
        if config.autoconnect {
            "yes".to_string()
        } else {
            "no".to_string()
        },
        "connection.autoconnect-priority".to_string(),
        config.priority.to_string(),
        "ipv4.method".to_string(),
        config.ipv4_method,
        "ipv6.method".to_string(),
        config.ipv6_method,
    ];

    if !config.dns.is_empty() {
        args.push("ipv4.dns".to_string());
        args.push(config.dns);
    }

    if !config.ipv4_address.is_empty() {
        args.push("ipv4.addresses".to_string());
        args.push(config.ipv4_address);
    }
    if !config.ipv4_gateway.is_empty() {
        args.push("ipv4.gateway".to_string());
        args.push(config.ipv4_gateway);
    }

    if !config.ipv6_address.is_empty() {
        args.push("ipv6.addresses".to_string());
        args.push(config.ipv6_address);
    }
    if !config.ipv6_gateway.is_empty() {
        args.push("ipv6.gateway".to_string());
        args.push(config.ipv6_gateway);
    }

    // Determine connection type for MAC address field
    let is_eth = Command::new("nmcli")
        .args(["-t", "-f", "connection.type", "connection", "show", ssid])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("ethernet"))
        .unwrap_or(false);

    if !config.mac_address.is_empty() {
        let field = if is_eth {
            "802-3-ethernet.cloned-mac-address"
        } else {
            "802-11-wireless.cloned-mac-address"
        };
        args.push(field.to_string());
        args.push(config.mac_address);
    }

    let out = Command::new("nmcli")
        .args(&args)
        .output()
        .map_err(|e| format!("cannot run nmcli: {}", e))?;
    if !out.status.success() {
        return Err(nmcli_error(&out));
    }
    if !is_eth && !config.password.is_empty() {
        crate::nm_dbus::set_wifi_password(ssid, &config.password)?;
    }
    Ok(())
}

pub fn get_ping(host: &str) -> Option<u64> {
    let output = Command::new("ping")
        .env("LC_ALL", "C")
        .args(["-c", "1", "-W", "1", host])
        .output()
        .ok()?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(time_part) = stdout.split("time=").nth(1)
            && let Some(ms_str) = time_part.split_whitespace().next()
        {
            return ms_str.parse::<f64>().ok().map(|f| f as u64);
        }
    }
    None
}

pub fn is_wifi_enabled() -> bool {
    Command::new("nmcli")
        .args(["radio", "wifi"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "enabled")
        .unwrap_or(true)
}

pub fn get_active_interface() -> Option<String> {
    let output = Command::new("nmcli")
        .args(["-t", "-f", "DEVICE,TYPE,STATE", "dev"])
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut wifi_iface = None;

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() >= 3 && parts[2] == "connected" {
            if parts[1] == "802-3-ethernet" || parts[1] == "ethernet" {
                return Some(parts[0].to_string());
            }
            if parts[1] == "wifi" {
                wifi_iface = Some(parts[0].to_string());
            }
        }
    }
    wifi_iface
}

/// (rx_bytes, tx_bytes) for an interface, from /proc/net/dev.
pub fn get_interface_stats(iface: &str) -> Option<(u64, u64)> {
    parse_proc_net_dev(&std::fs::read_to_string("/proc/net/dev").ok()?, iface)
}

/// Lines look like `  wlan0: 1234 56 ...`. Match the name before the colon
/// exactly: a substring match let "eth0" pick up "veth0..." rows.
fn parse_proc_net_dev(content: &str, iface: &str) -> Option<(u64, u64)> {
    content.lines().find_map(|line| {
        let (name, counters) = line.split_once(':')?;
        if name.trim() != iface {
            return None;
        }
        let fields: Vec<&str> = counters.split_whitespace().collect();
        // Receive: bytes packets errs drop fifo frame compressed multicast,
        // then Transmit: bytes ...
        Some((fields.first()?.parse().ok()?, fields.get(8)?.parse().ok()?))
    })
}

/// Connect to a network: with a password over D-Bus (see nm_dbus), without
/// one by bringing up the saved profile, falling back to a fresh connect for
/// an open network.
pub fn connect_to_wifi(ssid: &str, password: Option<&str>) -> Result<(), String> {
    if let Some(pass) = password {
        return crate::nm_dbus::connect_with_password(ssid, pass);
    }
    let run = |args: &[&str]| {
        Command::new("nmcli")
            .args(args)
            .output()
            .map_err(|e| format!("cannot run nmcli: {}", e))
    };
    let saved = run(&["--wait", "15", "connection", "up", "id", ssid])?;
    if saved.status.success() {
        return Ok(());
    }
    let fresh = run(&["--wait", "15", "device", "wifi", "connect", ssid])?;
    if fresh.status.success() {
        Ok(())
    } else {
        Err(nmcli_error(&fresh))
    }
}

/// Wi-Fi interfaces NetworkManager has connected or is connecting, e.g. ["wlan0"].
fn active_wifi_devices() -> Vec<String> {
    let Ok(output) = Command::new("nmcli")
        .args(["-t", "-f", "DEVICE,TYPE,STATE", "device"])
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(parse_nmcli_line)
        // STATE is "connected", "connected (externally)", "connecting (...)".
        .filter(|p| p.len() >= 3 && p[1] == "wifi" && p[2].starts_with("connect"))
        .map(|p| p[0].clone())
        .collect()
}

/// Disconnect every active Wi-Fi interface.
///
/// `nmcli device disconnect` takes an interface name. This used to pass the
/// literal "wifi", which is a device TYPE, not a device -- nmcli answered
/// "Device 'wifi' not found" and the disconnect never worked from either the
/// GUI or the CLI. NetworkManager also keeps a device disconnected this way
/// from auto-reconnecting until the user connects it again, which is the
/// behaviour a manual disconnect wants.
pub fn disconnect_wifi() -> Result<(), String> {
    let devices = active_wifi_devices();
    if devices.is_empty() {
        return Err("No active Wi-Fi connection".to_string());
    }
    let failed: Vec<&String> = devices
        .iter()
        .filter(|dev| {
            !Command::new("nmcli")
                .args(["device", "disconnect", dev.as_str()])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        })
        .collect();
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Could not disconnect {}",
            failed
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }
}

pub fn toggle_wifi_power(enabled: bool) -> bool {
    let state = if enabled { "on" } else { "off" };
    Command::new("nmcli")
        .args(["radio", "wifi", state])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(name: &str, ctype: &str, state: &str, device: &str) -> Connection {
        Connection {
            name: name.into(),
            ctype: ctype.into(),
            state: state.into(),
            device: device.into(),
        }
    }

    #[test]
    fn nmcli_terse_lines_honour_escapes() {
        assert_eq!(
            parse_nmcli_line(r"Cafe\:5G:AA\:BB:70"),
            ["Cafe:5G", "AA:BB", "70"]
        );
        assert_eq!(parse_nmcli_line("a::c"), ["a", "", "c"]);
    }

    #[test]
    fn proc_net_dev_matches_exact_interface() {
        let content = "Inter-|   Receive |  Transmit
 face |bytes packets errs drop fifo frame compressed multicast|bytes packets
 veth0abc: 999 1 0 0 0 0 0 0 888 1 0 0 0 0 0 0
    eth0: 1000 2 0 0 0 0 0 0 2000 3 0 0 0 0 0 0
";
        assert_eq!(parse_proc_net_dev(content, "eth0"), Some((1000, 2000)));
        assert_eq!(parse_proc_net_dev(content, "wlan0"), None);
    }

    #[test]
    fn builds_network_list() {
        let conns = [
            conn("Home", "802-11-wireless", "activated", "wlan0"),
            conn("Office LAN", "802-3-ethernet", "activated", "enp1s0"),
            conn("Old LAN", "802-3-ethernet", "", ""),
            conn("wg0", "wireguard", "", ""),
        ];
        let row = |ssid: &str, bssid: &str, signal: &str, inuse: &str| {
            vec![
                ssid.to_string(),
                bssid.to_string(),
                signal.to_string(),
                "WPA2".to_string(),
                inuse.to_string(),
            ]
        };
        let rows = [
            row("Cafe", "11", "80", ""),
            // Two APs for Home: the in-use one is weaker and listed second.
            row("Home", "21", "90", ""),
            row("Home", "22", "40", "*"),
            row("", "31", "99", ""), // hidden
            row("Home", "23", "95", ""),
        ];
        let nets = build_networks(&conns, &rows);
        let names: Vec<&str> = nets.iter().map(|n| n.ssid.as_str()).collect();
        // Connected (wired before Wi-Fi), then saved, then by signal.
        assert_eq!(names, ["Office LAN", "Home", "Cafe"]);
        let home = &nets[1];
        assert!(home.connected, "in use via its second access point");
        assert!(home.saved);
        assert_eq!(
            (home.signal, home.bssid.as_str()),
            (95, "23"),
            "strongest AP kept"
        );
        assert!(!nets[2].connected && !nets[2].saved);
    }

    #[test]
    fn vpn_rows_are_picked_from_connections() {
        let conns = [
            conn("wg0", "wireguard", "activated", "wg0"),
            conn("Home", "802-11-wireless", "", ""),
        ];
        let vpns: Vec<_> = vpns_from(&conns)
            .into_iter()
            .filter(|v| v.vpn_type == "wireguard")
            .collect();
        assert_eq!(vpns.len(), 1);
        assert!(vpns[0].active);
    }
}
