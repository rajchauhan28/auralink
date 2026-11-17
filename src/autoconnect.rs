//! Bluetooth auto-connect, shared by the headless daemon
//! (`auralink-bt daemon`, run by auralink-bt-daemon.service) and the
//! `auralink-bt` window, which runs it only while the daemon is not active.
//!
//! Policy:
//! - Only **audio** devices (headsets, headphones, speakers) are
//!   auto-connected, and only **one at a time**. Mice and keyboards reconnect
//!   on their own. Treating "any device connected" as done meant a trusted
//!   BLE mouse kept the daemon from ever reconnecting earbuds.
//! - A device the user **disconnected by hand** through AuraLink is left alone
//!   until they connect it again (by any means) or log out. See
//!   [`manual_disconnect`].
//! - Nothing happens while Bluetooth is powered off; the adapter is never
//!   switched on behind the user's back.
//! - Discovery runs only while waiting for a device. It competes with A2DP for
//!   the radio and makes playback stutter.

use crate::bt_backend::{self, BluetoothDevice, Snapshot};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

/// Poll cadence while waiting for a device to come into range.
pub const IDLE_POLL: Duration = Duration::from_secs(8);
/// Poll cadence once an audio device is connected; only watching for a drop.
pub const CONNECTED_POLL: Duration = Duration::from_secs(15);
/// Per-device backoff so a device that is simply switched off is not paged
/// on every tick.
const RETRY_COOLDOWN: Duration = Duration::from_secs(30);
/// A manual-disconnect marker younger than this is never cleared, even if the
/// device still looks connected: the disconnect may still be in flight.
const MARKER_GRACE: Duration = Duration::from_secs(60);

// ---------------------------------------------------------------------------
// Manual-disconnect markers
//
// One empty file per device under $XDG_RUNTIME_DIR/auralink/bt-manual-disconnect/.
// The runtime dir is a per-user tmpfs that is shared by the window and the
// systemd user service and wiped at logout, which gives exactly the lifetime
// wanted. Disconnects made with other tools (desktop applets, bluetoothctl)
// leave no marker, so the daemon will still reconnect those devices.

/// The marker directory; a struct so tests can point it elsewhere.
struct Markers(PathBuf);

impl Markers {
    fn open() -> Option<Markers> {
        let base = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).or_else(|| {
            // Not set outside a login session; fall back to the standard path.
            let status = std::fs::read_to_string("/proc/self/status").ok()?;
            let uid = status.lines().find_map(|l| l.strip_prefix("Uid:"))?.split_whitespace().next()?.to_string();
            let dir = PathBuf::from(format!("/run/user/{}", uid));
            dir.is_dir().then_some(dir)
        })?;
        Some(Markers(base.join("auralink").join("bt-manual-disconnect")))
    }

    fn path(&self, address: &str) -> PathBuf {
        self.0.join(address.to_ascii_uppercase())
    }

    fn is_set(&self, address: &str) -> bool {
        self.path(address).exists()
    }

    fn set(&self, address: &str) {
        let _ = std::fs::create_dir_all(&self.0);
        let _ = std::fs::write(self.path(address), b"");
    }

    fn clear(&self, address: &str) {
        let _ = std::fs::remove_file(self.path(address));
    }

    /// Clear the marker only if it is older than `grace`.
    fn clear_if_older_than(&self, address: &str, grace: Duration) {
        let path = self.path(address);
        let age = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok());
        if age.is_some_and(|a| a > grace) {
            let _ = std::fs::remove_file(path);
        }
    }
}

pub fn is_suppressed(address: &str) -> bool {
    Markers::open().is_some_and(|m| m.is_set(address))
}

fn suppress(address: &str) {
    if let Some(m) = Markers::open() {
        m.set(address);
    }
}

/// Re-enable auto-connect for a device. Call when the user connects, pairs or
/// removes it.
pub fn unsuppress(address: &str) {
    if let Some(m) = Markers::open() {
        m.clear(address);
    }
}

/// Clear the marker of a device that is connected again, unless the marker is
/// so new that the disconnect that created it may not have landed yet.
fn unsuppress_if_reconnected(address: &str) {
    if let Some(m) = Markers::open() {
        m.clear_if_older_than(address, MARKER_GRACE);
    }
}

/// Disconnect a device on the user's behalf and keep auto-connect from undoing
/// it. The marker is written first so the daemon cannot slip a reconnect in
/// while the disconnect is running, and withdrawn if the disconnect fails.
pub fn manual_disconnect(address: &str) -> Result<(), String> {
    suppress(address);
    let result = bt_backend::disconnect(address);
    if result.is_err() {
        unsuppress(address);
    }
    result
}

/// Connect a device on the user's behalf; re-enables auto-connect for it.
pub fn manual_connect(address: &str) -> Result<(), String> {
    unsuppress(address);
    bt_backend::connect(address)
}

// ---------------------------------------------------------------------------
// The loop

#[derive(Default)]
pub struct AutoConnector {
    last_attempt: HashMap<String, Instant>,
    /// Devices whose failed attempt has been logged; later failures stay
    /// quiet so an absent device does not write a log line every 30s.
    reported_unreachable: HashSet<String>,
    /// Address of the audio device last seen connected, to log changes once.
    connected: Option<String>,
}

impl AutoConnector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Run one pass and return how long to wait before the next. Progress is
    /// reported through `log`.
    pub fn tick(&mut self, log: &mut dyn FnMut(String)) -> Duration {
        let snap = Snapshot::read();

        if !snap.powered() {
            bt_backend::stop_scan();
            self.connected = None;
            return IDLE_POLL;
        }

        for dev in snap.connected() {
            unsuppress_if_reconnected(&dev.address);
        }

        if let Some(dev) = snap.connected().find(|d| d.is_audio) {
            if self.connected.as_deref() != Some(dev.address.as_str()) {
                log(format!("{} ({}) connected", dev.name, dev.address));
                self.connected = Some(dev.address.clone());
            }
            if bt_backend::scan_running() {
                log("stopping discovery while audio is connected".to_string());
                bt_backend::stop_scan();
            }
            self.last_attempt.clear();
            self.reported_unreachable.clear();
            return CONNECTED_POLL;
        }
        self.connected = None;

        let candidates: Vec<&BluetoothDevice> = snap
            .devices
            .iter()
            .filter(|d| d.is_audio && d.trusted && !is_suppressed(&d.address))
            .collect();
        if candidates.is_empty() {
            // Nothing to reconnect to; do not keep the radio scanning.
            bt_backend::stop_scan();
            return IDLE_POLL;
        }

        // A trusted device that has been out of range is not reliably pageable
        // until the adapter has seen it advertise again, so keep a discovery
        // session alive while waiting. bluetoothctl ends each session after
        // SCAN_SESSION_SECS; scan_running() going false is the cue to renew.
        if !bt_backend::scan_running() {
            bt_backend::start_scan();
        }

        let now = Instant::now();
        for dev in candidates {
            if self.last_attempt.get(&dev.address).is_some_and(|t| now.duration_since(*t) < RETRY_COOLDOWN) {
                continue;
            }
            // Each attempt can take seconds: re-check that nothing connected
            // and that the user has not disconnected this device meanwhile.
            if Snapshot::read().connected().any(|d| d.is_audio) || is_suppressed(&dev.address) {
                break;
            }

            self.last_attempt.insert(dev.address.clone(), now);
            match bt_backend::connect(&dev.address) {
                Ok(()) => {
                    log(format!("auto-connected {} ({})", dev.name, dev.address));
                    self.connected = Some(dev.address.clone());
                    self.reported_unreachable.remove(&dev.address);
                    bt_backend::stop_scan();
                    // Give BlueZ a moment to register the card with PipeWire
                    // before choosing a profile on it.
                    std::thread::sleep(Duration::from_secs(2));
                    if bt_backend::force_a2dp_profile(&dev.address) {
                        log(format!("selected A2DP profile for {}", dev.name));
                    }
                    return CONNECTED_POLL;
                }
                Err(e) => {
                    if self.reported_unreachable.insert(dev.address.clone()) {
                        log(format!(
                            "{} ({}) did not connect: {}; retrying every {}s",
                            dev.name,
                            dev.address,
                            e,
                            RETRY_COOLDOWN.as_secs()
                        ));
                    }
                }
            }
        }
        IDLE_POLL
    }
}

/// The headless daemon: `auralink-bt daemon`.
pub fn run_daemon() -> ! {
    eprintln!("=== AuraLink Bluetooth Auto-Connect Daemon Started ===");
    let _ = bt_backend::ensure_switch_on_connect_module();
    let mut connector = AutoConnector::new();
    loop {
        let wait = connector.tick(&mut |msg| eprintln!("Daemon: {}", msg));
        std::thread::sleep(wait);
    }
}

/// Is the headless daemon running? The window defers to it so two processes
/// do not page the same devices and race each other's discovery sessions.
/// `systemctl is-active` exits 0 active, 3 inactive, 4 no such unit.
pub fn daemon_active() -> bool {
    std::process::Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", "auralink-bt-daemon.service"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_round_trip() {
        let dir = std::env::temp_dir().join(format!("auralink-test-{}", std::process::id()));
        let m = Markers(dir.clone());

        let mac = "aa:bb:cc:dd:ee:ff";
        assert!(!m.is_set(mac));
        m.set(mac);
        assert!(m.is_set("AA:BB:CC:DD:EE:FF"), "case-insensitive");
        // A fresh marker is inside the grace period and survives...
        m.clear_if_older_than(mac, MARKER_GRACE);
        assert!(m.is_set(mac));
        // ...an old one does not.
        std::thread::sleep(Duration::from_millis(20));
        m.clear_if_older_than(mac, Duration::from_millis(10));
        assert!(!m.is_set(mac));

        m.set(mac);
        m.clear(mac);
        assert!(!m.is_set(mac));
        let _ = std::fs::remove_dir_all(dir);
    }
}
