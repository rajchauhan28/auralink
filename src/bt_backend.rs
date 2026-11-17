//! Bluetooth backend.
//!
//! **State** comes from ONE D-Bus call: BlueZ's
//! `ObjectManager.GetManagedObjects`, read through `busctl --json`. It returns
//! every adapter, device, flag and battery in a few milliseconds. This
//! replaced a `bluetoothctl info` per device, a `busctl tree` walk for
//! batteries and a `dbus-send` per device for the trust flag -- dozens of
//! processes per poll, repeated every few seconds by the GUI, the daemon and
//! every Waybar refresh.
//!
//! **Actions** are single D-Bus method calls or property writes through
//! `busctl`. busctl returns the moment BlueZ replies and its exit status is
//! the real outcome. bluetoothctl did neither: it exits 0 even on failure, and
//! `bluetoothctl --timeout N <cmd>` always runs for the full N seconds even
//! when the command finished instantly (measured: `--timeout 4 show` takes
//! 4.0s against 6ms without it), so every disconnect took 10s and every
//! auto-connect attempt 20s.
//!
//! bluetoothctl is still used for the two jobs that need its client session:
//! discovery (BlueZ ends a discovery session when its client exits) and
//! pairing (bluetoothctl registers the agent that answers pairing prompts).

// Shared with the `auralink` binary, which uses only the power controls.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const DEVICE_IFACE: &str = "org.bluez.Device1";
const ADAPTER_IFACE: &str = "org.bluez.Adapter1";
const BATTERY_IFACE: &str = "org.bluez.Battery1";

/// Service UUIDs that mark a device as something you listen through.
const AUDIO_UUIDS: [&str; 3] = [
    "0000110b-0000-1000-8000-00805f9b34fb", // A2DP Audio Sink
    "0000111e-0000-1000-8000-00805f9b34fb", // Hands-Free (HF unit)
    "00001108-0000-1000-8000-00805f9b34fb", // Headset (HS)
];

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
    /// Lowest component battery, kept for consumers of the older single field.
    pub battery: Option<i32>,
    pub batteries: Vec<Battery>,
    pub audio_profiles: Vec<AudioProfile>,
    /// BlueZ's icon hint, e.g. "audio-headset", "input-mouse".
    pub icon: String,
    /// A headset, headphones or speaker (see AUDIO_UUIDS).
    pub is_audio: bool,
    /// D-Bus object path, e.g. /org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF.
    #[serde(skip)]
    pub path: String,
    /// Object path of the adapter this device belongs to.
    #[serde(skip)]
    pub adapter: String,
}

#[derive(Debug, Clone)]
pub struct Adapter {
    pub path: String,
    pub powered: bool,
}

/// Everything BlueZ knows, read in one call.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub adapters: Vec<Adapter>,
    pub devices: Vec<BluetoothDevice>,
}

impl Snapshot {
    /// Read the current state. Empty if BlueZ (or busctl) is unavailable,
    /// which callers treat as "no adapter, no devices".
    pub fn read() -> Snapshot {
        let Ok(out) = Command::new("busctl")
            .args([
                "--system",
                "--json=short",
                "--timeout=5",
                "call",
                "org.bluez",
                "/",
                "org.freedesktop.DBus.ObjectManager",
                "GetManagedObjects",
            ])
            .output()
        else {
            return Snapshot::default();
        };
        if !out.status.success() {
            return Snapshot::default();
        }
        serde_json::from_slice::<Value>(&out.stdout)
            .map(|v| Snapshot::from_managed_objects(&v))
            .unwrap_or_default()
    }

    /// Parse busctl's JSON rendering of `a{oa{sa{sv}}}`:
    /// `{"type": ..., "data": [{path: {iface: {prop: {"type", "data"}}}}]}`.
    fn from_managed_objects(json: &Value) -> Snapshot {
        let mut snap = Snapshot::default();
        let Some(objects) = json
            .get("data")
            .and_then(|d| d.get(0))
            .and_then(Value::as_object)
        else {
            return snap;
        };

        let mut batteries: Vec<(&str, i32)> = Vec::new();
        for (path, ifaces) in objects {
            if let Some(a) = ifaces.get(ADAPTER_IFACE) {
                snap.adapters.push(Adapter {
                    path: path.clone(),
                    powered: prop_bool(a, "Powered"),
                });
            }
            if let Some(b) = ifaces.get(BATTERY_IFACE)
                && let Some(pct) = prop(b, "Percentage").and_then(Value::as_i64)
            {
                batteries.push((path.as_str(), pct as i32));
            }
            if let Some(d) = ifaces.get(DEVICE_IFACE) {
                let address = prop_str(d, "Address");
                if !is_valid_mac(&address) {
                    continue;
                }
                let icon = prop_str(d, "Icon");
                let uuids: Vec<&str> = prop(d, "UUIDs")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_str).collect())
                    .unwrap_or_default();
                let is_audio = icon.starts_with("audio-")
                    || uuids
                        .iter()
                        .any(|u| AUDIO_UUIDS.contains(&u.to_ascii_lowercase().as_str()));
                // Alias is what bluetoothctl and desktop applets show: the
                // user's rename if any, else the advertised name.
                let mut name = prop_str(d, "Alias");
                if name.is_empty() {
                    name = prop_str(d, "Name");
                }
                snap.devices.push(BluetoothDevice {
                    name,
                    address,
                    connected: prop_bool(d, "Connected"),
                    paired: prop_bool(d, "Paired"),
                    trusted: prop_bool(d, "Trusted"),
                    // Only present while the device is being discovered.
                    rssi: prop(d, "RSSI").and_then(Value::as_i64).unwrap_or(0) as i32,
                    battery: None,
                    batteries: Vec::new(),
                    audio_profiles: Vec::new(),
                    icon,
                    is_audio,
                    path: path.clone(),
                    adapter: prop_str(d, "Adapter"),
                });
            }
        }

        // Standard devices expose Battery1 on the device object itself; some
        // TWS earbuds expose one per component (left/right/case) on child
        // objects. Attach whichever exist.
        for dev in &mut snap.devices {
            let dev_id = dev.path.rsplit('/').next().unwrap_or_default().to_string();
            let child_prefix = format!("{}/", dev.path);
            for (path, pct) in &batteries {
                if *path == dev.path || path.starts_with(&child_prefix) {
                    dev.batteries.push(Battery {
                        label: battery_label(path, &dev_id),
                        percentage: *pct,
                    });
                }
            }
            // Whole-device battery first, then components alphabetically.
            dev.batteries.sort_by(|a, b| {
                (!a.label.is_empty(), &a.label).cmp(&(!b.label.is_empty(), &b.label))
            });
            dev.battery = dev.batteries.iter().map(|b| b.percentage).min();
        }

        snap.devices.sort_by(|a, b| {
            b.connected
                .cmp(&a.connected)
                .then_with(|| b.paired.cmp(&a.paired))
                .then_with(|| a.name.cmp(&b.name))
        });
        snap
    }

    pub fn powered(&self) -> bool {
        self.adapters.iter().any(|a| a.powered)
    }

    pub fn device(&self, address: &str) -> Option<&BluetoothDevice> {
        self.devices
            .iter()
            .find(|d| d.address.eq_ignore_ascii_case(address))
    }

    pub fn connected(&self) -> impl Iterator<Item = &BluetoothDevice> {
        self.devices.iter().filter(|d| d.connected)
    }

    /// Fill in PipeWire/PulseAudio card profiles for connected devices. One
    /// `pactl` call covers all of them, and none is made if nothing is
    /// connected.
    pub fn with_audio_profiles(mut self) -> Snapshot {
        if self.devices.iter().any(|d| d.connected) {
            let cards = pa_cards();
            for dev in self.devices.iter_mut().filter(|d| d.connected) {
                if let Some(card) = find_card(&cards, &dev.address) {
                    dev.audio_profiles = card
                        .profiles
                        .iter()
                        .map(|p| AudioProfile {
                            name: p.name.clone(),
                            description: p.description.clone(),
                            active: p.name == card.active_profile,
                            available: p.available,
                        })
                        .collect();
                }
            }
        }
        self
    }
}

fn prop<'a>(iface: &'a Value, name: &str) -> Option<&'a Value> {
    iface.get(name)?.get("data")
}

fn prop_bool(iface: &Value, name: &str) -> bool {
    prop(iface, name).and_then(Value::as_bool).unwrap_or(false)
}

fn prop_str(iface: &Value, name: &str) -> String {
    prop(iface, name)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

pub fn is_valid_mac(addr: &str) -> bool {
    let parts: Vec<&str> = addr.split(':').collect();
    parts.len() == 6
        && parts
            .iter()
            .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()))
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
        let mut chars = suffix.chars();
        match chars.next() {
            Some(c) => c.to_uppercase().chain(chars).collect(),
            None => String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Queries

/// Every known device, with audio profiles for the connected ones.
pub fn list_devices() -> Vec<BluetoothDevice> {
    Snapshot::read().with_audio_profiles().devices
}

pub fn list_connected_devices() -> Vec<BluetoothDevice> {
    Snapshot::read()
        .with_audio_profiles()
        .devices
        .into_iter()
        .filter(|d| d.connected)
        .collect()
}

pub fn is_powered() -> bool {
    Snapshot::read().powered()
}

pub fn has_adapter() -> bool {
    !Snapshot::read().adapters.is_empty()
}

// ---------------------------------------------------------------------------
// Actions

/// `busctl --system --timeout=N <verb> org.bluez <args...>`, mapping a D-Bus
/// error to its message ("Call failed: " stripped).
fn busctl(timeout_secs: u32, verb: &str, args: &[&str]) -> Result<(), String> {
    let timeout = format!("--timeout={}", timeout_secs);
    let out = Command::new("busctl")
        .args(["--system", timeout.as_str(), verb, "org.bluez"])
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run busctl: {}", e))?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let err = err.trim();
    Err(err.strip_prefix("Call failed: ").unwrap_or(err).to_string())
}

/// Resolve a MAC address to its BlueZ device, on whichever adapter has it.
fn find_device(address: &str) -> Result<BluetoothDevice, String> {
    if !is_valid_mac(address) {
        return Err(format!(
            "'{}' is not a Bluetooth address (expected AA:BB:CC:DD:EE:FF)",
            address
        ));
    }
    Snapshot::read()
        .device(address)
        .cloned()
        .ok_or_else(|| format!("{} is not a known device", address))
}

pub fn connect(address: &str) -> Result<(), String> {
    let dev = find_device(address)?;
    // 20s: long enough for a paged device to answer. An absent device fails
    // sooner on its own (BlueZ's page timeout is ~5s).
    busctl(20, "call", &[&dev.path, DEVICE_IFACE, "Connect"])
}

pub fn disconnect(address: &str) -> Result<(), String> {
    let dev = find_device(address)?;
    busctl(10, "call", &[&dev.path, DEVICE_IFACE, "Disconnect"])
}

pub fn trust(address: &str, enable: bool) -> Result<(), String> {
    let dev = find_device(address)?;
    busctl(
        5,
        "set-property",
        &[
            &dev.path,
            DEVICE_IFACE,
            "Trusted",
            "b",
            if enable { "true" } else { "false" },
        ],
    )
}

/// Unpair and forget the device.
pub fn remove(address: &str) -> Result<(), String> {
    let dev = find_device(address)?;
    busctl(
        10,
        "call",
        &[&dev.adapter, ADAPTER_IFACE, "RemoveDevice", "o", &dev.path],
    )
}

/// Power every adapter on or off.
pub fn set_power(enable: bool) -> Result<(), String> {
    let snap = Snapshot::read();
    if snap.adapters.is_empty() {
        return Err("no Bluetooth adapter found".to_string());
    }
    let value = if enable { "true" } else { "false" };
    let errors: Vec<String> = snap
        .adapters
        .iter()
        .filter_map(|a| {
            busctl(
                5,
                "set-property",
                &[&a.path, ADAPTER_IFACE, "Powered", "b", value],
            )
            .err()
        })
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Pair through bluetoothctl, whose session registers the agent that answers
/// the device's pairing request.
pub fn pair(address: &str) -> Result<(), String> {
    if !is_valid_mac(address) {
        return Err(format!("'{}' is not a Bluetooth address", address));
    }
    let mut cmd = Command::new("bluetoothctl");
    cmd.args(["pair", address]);
    // Pairing may wait for confirmation on the peer, so allow 30s.
    match output_within(cmd, Duration::from_secs(30)) {
        None => Err("cannot run bluetoothctl (is bluez-utils installed?)".to_string()),
        Some(text) if text.contains("Pairing successful") => Ok(()),
        Some(text) => Err(text
            .lines()
            .rev()
            .find(|l| {
                l.contains("Failed") || l.contains("not available") || l.contains("org.bluez.Error")
            })
            .unwrap_or("pairing did not complete")
            .trim()
            .to_string()),
    }
}

/// Run `cmd` to completion, killing it if it outlives `limit`. Returns its
/// combined stdout and stderr, or None if it could not be started.
///
/// This replaces `bluetoothctl --timeout`, which makes bluetoothctl wait out
/// the whole timeout even after the command has finished.
fn output_within(mut cmd: Command, limit: Duration) -> Option<String> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    // Drain both pipes on their own threads so a chatty child cannot block
    // on a full pipe while we wait for it.
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut buf);
            }
            buf
        })
    };
    let out = drain(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let err = drain(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );

    let deadline = Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
    let mut text = String::from_utf8_lossy(&out.join().unwrap_or_default()).into_owned();
    text.push_str(&String::from_utf8_lossy(&err.join().unwrap_or_default()));
    Some(text)
}

// ---------------------------------------------------------------------------
// Discovery

/// The `bluetoothctl ... scan on` child, if discovery is running.
///
/// Discovery needs a bluetoothctl session held open, so it has to be a
/// long-lived child. It is tracked here so it can be killed AND REAPED: a
/// dropped handle leaks a zombie, and the auto-connect daemon restarts
/// discovery every time the session expires.
static SCAN_CHILD: Mutex<Option<Child>> = Mutex::new(None);

/// How long a single discovery session runs before bluetoothctl exits. This
/// is the one place `--timeout` is wanted: it is the session length.
pub const SCAN_SESSION_SECS: u64 = 60;

/// Start a discovery session. Does NOT power the adapter on: the daemon calls
/// this unattended, and powering on here used to undo the user switching
/// Bluetooth off (or airplane mode) within a minute.
pub fn start_scan() -> bool {
    stop_scan();
    match Command::new("bluetoothctl")
        .args(["--timeout", &SCAN_SESSION_SECS.to_string(), "scan", "on"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
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
    let Ok(mut slot) = SCAN_CHILD.lock() else {
        return false;
    };
    match slot.as_mut() {
        // try_wait reaps it the moment it exits, so the session ending does
        // not leave a zombie behind either.
        Some(child) => match child.try_wait() {
            Ok(None) => true,
            _ => {
                *slot = None;
                false
            }
        },
        None => false,
    }
}

/// End our discovery session. BlueZ tracks discovery per client and stops it
/// when the client goes away, so killing our own bluetoothctl is the whole
/// job. (A separate `bluetoothctl scan off` cannot stop another client's
/// session, and `pkill -f` would also hit scans the user started by hand.)
pub fn stop_scan() {
    if let Ok(mut slot) = SCAN_CHILD.lock()
        && let Some(mut child) = slot.take()
    {
        let _ = child.kill();
        let _ = child.wait();
    }
}

// ---------------------------------------------------------------------------
// Audio (PipeWire / PulseAudio cards)

struct PaProfile {
    name: String,
    description: String,
    priority: i64,
    available: bool,
}

struct PaCard {
    name: String,
    active_profile: String,
    profiles: Vec<PaProfile>,
}

/// All sound cards from `pactl --format=json list cards`, profiles sorted by
/// priority (best first). Empty if pactl is missing or too old for JSON.
fn pa_cards() -> Vec<PaCard> {
    Command::new("pactl")
        .args(["--format=json", "list", "cards"])
        .output()
        .ok()
        .and_then(|o| serde_json::from_slice::<Value>(&o.stdout).ok())
        .map(|v| parse_pa_cards(&v))
        .unwrap_or_default()
}

fn parse_pa_cards(json: &Value) -> Vec<PaCard> {
    let Some(cards) = json.as_array() else {
        return Vec::new();
    };
    cards
        .iter()
        .filter_map(|c| {
            let mut profiles: Vec<PaProfile> = c
                .get("profiles")?
                .as_object()?
                .iter()
                .map(|(name, p)| PaProfile {
                    name: name.clone(),
                    description: p
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    priority: p.get("priority").and_then(Value::as_i64).unwrap_or(0),
                    available: p.get("available").and_then(Value::as_bool).unwrap_or(true),
                })
                .collect();
            profiles.sort_by(|a, b| {
                b.priority
                    .cmp(&a.priority)
                    .then_with(|| a.name.cmp(&b.name))
            });
            Some(PaCard {
                name: c.get("name")?.as_str()?.to_string(),
                active_profile: c
                    .get("active_profile")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                profiles,
            })
        })
        .collect()
}

/// The `bluez_card.AA_BB_...` card for a device.
fn find_card<'a>(cards: &'a [PaCard], address: &str) -> Option<&'a PaCard> {
    let suffix = address.replace(':', "_").to_ascii_uppercase();
    cards.iter().find(|c| {
        c.name.starts_with("bluez_card.") && c.name.to_ascii_uppercase().ends_with(&suffix)
    })
}

fn set_card_profile(card: &str, profile: &str) -> bool {
    Command::new("pactl")
        .args(["set-card-profile", card, profile])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn set_audio_profile(address: &str, profile_name: &str) -> bool {
    let cards = pa_cards();
    find_card(&cards, address).is_some_and(|card| set_card_profile(&card.name, profile_name))
}

/// Switch a freshly connected headset to its best available A2DP profile.
/// Headsets often come up on HSP/HFP (phone-call quality) instead.
pub fn force_a2dp_profile(address: &str) -> bool {
    let cards = pa_cards();
    let Some(card) = find_card(&cards, address) else {
        return false;
    };
    // Profiles are sorted best-first, so the first available A2DP sink wins
    // (e.g. AAC "a2dp-sink" at 133 over "a2dp-sink-sbc" at 132).
    let Some(best) = card
        .profiles
        .iter()
        .find(|p| p.available && p.name.starts_with("a2dp-sink"))
    else {
        return false;
    };
    card.active_profile == best.name || set_card_profile(&card.name, &best.name)
}

/// Load PipeWire/PulseAudio's module-switch-on-connect so a headset becomes
/// the default output when it connects.
pub fn ensure_switch_on_connect_module() -> bool {
    let loaded = Command::new("pactl")
        .args(["list", "modules", "short"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("module-switch-on-connect"))
        .unwrap_or(false);
    loaded
        || Command::new("pactl")
            .args(["load-module", "module-switch-on-connect"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Trimmed from a real `busctl --json=short call org.bluez / ...
    // GetManagedObjects` on BlueZ 5.88: a BLE mouse, a connected headset, an
    // out-of-range headset, and a component battery on a child object.
    const MANAGED_OBJECTS: &str = r#"{"type":"a{oa{sa{sv}}}","data":[{
      "/org/bluez":{"org.bluez.AgentManager1":{}},
      "/org/bluez/hci0":{"org.bluez.Adapter1":{"Powered":{"type":"b","data":true}}},
      "/org/bluez/hci0/dev_CA_78_43_91_63_97":{
        "org.bluez.Device1":{"Address":{"type":"s","data":"CA:78:43:91:63:97"},"Alias":{"type":"s","data":"BT5.1 Mouse"},
          "Icon":{"type":"s","data":"input-mouse"},"Paired":{"type":"b","data":true},"Trusted":{"type":"b","data":true},
          "Connected":{"type":"b","data":true},"Adapter":{"type":"o","data":"/org/bluez/hci0"},
          "UUIDs":{"type":"as","data":["00001812-0000-1000-8000-00805f9b34fb"]}},
        "org.bluez.Battery1":{"Percentage":{"type":"y","data":65}}},
      "/org/bluez/hci0/dev_28_04_C6_A8_22_FE":{
        "org.bluez.Device1":{"Address":{"type":"s","data":"28:04:C6:A8:22:FE"},"Name":{"type":"s","data":"Buds"},
          "Icon":{"type":"s","data":"audio-headset"},"Paired":{"type":"b","data":true},"Trusted":{"type":"b","data":true},
          "Connected":{"type":"b","data":true},"Adapter":{"type":"o","data":"/org/bluez/hci0"},"RSSI":{"type":"n","data":-60}}},
      "/org/bluez/hci0/dev_28_04_C6_A8_22_FE/battery_case":{"org.bluez.Battery1":{"Percentage":{"type":"y","data":80}}},
      "/org/bluez/hci0/dev_28_04_C6_A8_22_FE/battery_left":{"org.bluez.Battery1":{"Percentage":{"type":"y","data":20}}},
      "/org/bluez/hci0/dev_88_0E_85_91_AF_F2":{
        "org.bluez.Device1":{"Address":{"type":"s","data":"88:0E:85:91:AF:F2"},"Alias":{"type":"s","data":"Speaker"},
          "Paired":{"type":"b","data":true},"Trusted":{"type":"b","data":false},"Connected":{"type":"b","data":false},
          "Adapter":{"type":"o","data":"/org/bluez/hci0"},
          "UUIDs":{"type":"as","data":["0000110B-0000-1000-8000-00805F9B34FB"]}}}
    }]}"#;

    fn snapshot() -> Snapshot {
        Snapshot::from_managed_objects(&serde_json::from_str(MANAGED_OBJECTS).unwrap())
    }

    #[test]
    fn parses_adapters_and_devices() {
        let s = snapshot();
        assert_eq!(s.adapters.len(), 1);
        assert!(s.powered());
        assert_eq!(s.devices.len(), 3);
        // Connected first, then by name.
        let names: Vec<&str> = s.devices.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["BT5.1 Mouse", "Buds", "Speaker"]);
    }

    #[test]
    fn classifies_audio_devices() {
        let s = snapshot();
        assert!(!s.device("CA:78:43:91:63:97").unwrap().is_audio, "mouse");
        assert!(
            s.device("28:04:C6:A8:22:FE").unwrap().is_audio,
            "audio-* icon"
        );
        // No icon, but an (upper-case) A2DP sink UUID.
        assert!(s.device("88:0e:85:91:af:f2").unwrap().is_audio, "A2DP UUID");
    }

    #[test]
    fn attaches_device_and_component_batteries() {
        let s = snapshot();
        let mouse = s.device("CA:78:43:91:63:97").unwrap();
        assert_eq!(mouse.batteries.len(), 1);
        assert_eq!(mouse.batteries[0].label, "");
        assert_eq!(mouse.battery, Some(65));

        let buds = s.device("28:04:C6:A8:22:FE").unwrap();
        let labels: Vec<&str> = buds.batteries.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(labels, ["Case", "Left"]);
        assert_eq!(buds.battery, Some(20), "lowest component");
        assert_eq!(buds.rssi, -60);
        assert_eq!(buds.adapter, "/org/bluez/hci0");
    }

    #[test]
    fn empty_or_garbage_json_is_an_empty_snapshot() {
        let s = Snapshot::from_managed_objects(&serde_json::json!({"oops": 1}));
        assert!(s.adapters.is_empty() && s.devices.is_empty() && !s.powered());
    }

    #[test]
    fn picks_best_available_a2dp_profile() {
        let json = serde_json::json!([
            {"name": "alsa_card.pci", "active_profile": "HiFi", "profiles": {"HiFi": {"priority": 1}}},
            {"name": "bluez_card.28_04_C6_A8_22_FE", "active_profile": "headset-head-unit",
             "profiles": {
                "a2dp-sink-sbc": {"description": "SBC", "priority": 132, "available": true},
                "a2dp-sink": {"description": "AAC", "priority": 133, "available": true},
                "a2dp-sink-ldac": {"description": "LDAC", "priority": 140, "available": false},
                "headset-head-unit": {"description": "HFP", "priority": 6, "available": true}
             }}
        ]);
        let cards = parse_pa_cards(&json);
        let card = find_card(&cards, "28:04:c6:a8:22:fe").expect("case-insensitive card match");
        let best = card
            .profiles
            .iter()
            .find(|p| p.available && p.name.starts_with("a2dp-sink"))
            .unwrap();
        assert_eq!(
            best.name, "a2dp-sink",
            "unavailable LDAC skipped, AAC beats SBC"
        );
        assert!(find_card(&cards, "00:00:00:00:00:00").is_none());
    }

    #[test]
    fn mac_validation() {
        assert!(is_valid_mac("28:04:C6:A8:22:FE"));
        assert!(!is_valid_mac("28:04:C6:A8:22"));
        assert!(!is_valid_mac("28:04:C6:A8:22:FG"));
        assert!(!is_valid_mac("hci0"));
    }

    #[test]
    fn output_within_returns_early_and_kills_on_deadline() {
        let start = Instant::now();
        let mut quick = Command::new("sh");
        quick.args(["-c", "echo hi; echo err >&2"]);
        let text = output_within(quick, Duration::from_secs(10)).unwrap();
        assert!(text.contains("hi") && text.contains("err"));
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "must not wait out the limit"
        );

        let start = Instant::now();
        let mut slow = Command::new("sleep");
        slow.arg("30");
        output_within(slow, Duration::from_millis(300)).unwrap();
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "must kill at the deadline"
        );
    }
}
