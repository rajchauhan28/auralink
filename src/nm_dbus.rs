//! NetworkManager operations that carry a Wi-Fi password, done over D-Bus.
//!
//! Everything else in nm_backend shells out to nmcli. These two did too,
//! with the password as an argument (`nmcli dev wifi connect SSID password
//! PASS`, `nmcli connection modify ... 802-11-wireless-security.psk PASS`).
//! A process's arguments are world-readable through /proc/<pid>/cmdline,
//! so any local user running `ps` while nmcli worked could read the
//! password. Over D-Bus the password travels inside the message on the
//! system bus socket, which only NetworkManager receives.
//!
//! nmcli has `passwd-file` for `connection up`, but nothing equivalent for
//! `connection modify`, so it could not cover the Advanced dialog.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use zbus::blocking::Connection;
use zbus::blocking::Proxy;
use zbus::blocking::proxy::Builder;
use zbus::proxy::CacheProperties;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

const NM: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const NM_IFACE: &str = "org.freedesktop.NetworkManager";
const SETTINGS_PATH: &str = "/org/freedesktop/NetworkManager/Settings";
const SETTINGS_IFACE: &str = "org.freedesktop.NetworkManager.Settings";
const CONN_IFACE: &str = "org.freedesktop.NetworkManager.Settings.Connection";
const DEVICE_IFACE: &str = "org.freedesktop.NetworkManager.Device";
const WIRELESS_IFACE: &str = "org.freedesktop.NetworkManager.Device.Wireless";
const AP_IFACE: &str = "org.freedesktop.NetworkManager.AccessPoint";
const ACTIVE_IFACE: &str = "org.freedesktop.NetworkManager.Connection.Active";
const WSEC: &str = "802-11-wireless-security";

const NM_DEVICE_TYPE_WIFI: u32 = 2;
const NM_ACTIVE_CONNECTION_STATE_ACTIVATED: u32 = 2;
const NM_ACTIVE_CONNECTION_STATE_DEACTIVATED: u32 = 4;
const AP_FLAGS_PRIVACY: u32 = 0x1;
const AP_SEC_KEY_MGMT_PSK: u32 = 0x100;
const AP_SEC_KEY_MGMT_802_1X: u32 = 0x200;
const AP_SEC_KEY_MGMT_SAE: u32 = 0x400;

/// How long to wait for a connection to come up; nmcli used `--wait 15`.
const ACTIVATION_TIMEOUT: Duration = Duration::from_secs(20);

type Settings = HashMap<String, HashMap<String, OwnedValue>>;

fn err(e: zbus::Error) -> String {
    match e {
        // "org.freedesktop.NetworkManager.Settings.InvalidProperty: ..." ->
        // keep NetworkManager's own message, which is the useful part.
        zbus::Error::MethodError(_, Some(msg), _) => msg,
        other => other.to_string(),
    }
}

fn proxy<'a>(conn: &Connection, path: &str, iface: &'a str) -> Result<Proxy<'a>, String> {
    Builder::<Proxy>::new(conn)
        .destination(NM)
        .and_then(|b| b.path(path.to_string()))
        .and_then(|b| b.interface(iface))
        .map(|b| b.cache_properties(CacheProperties::No))
        .and_then(|b| b.build())
        .map_err(err)
}

fn system_bus() -> Result<Connection, String> {
    Connection::system().map_err(|e| format!("cannot reach the system bus: {}", e))
}

/// A visible access point for the SSID, on whichever Wi-Fi device sees it
/// strongest.
struct Target {
    device: OwnedObjectPath,
    ap: OwnedObjectPath,
    flags: u32,
    wpa_flags: u32,
    rsn_flags: u32,
}

fn find_access_point(conn: &Connection, ssid: &str) -> Result<Target, String> {
    let nm = proxy(conn, NM_PATH, NM_IFACE)?;
    let devices: Vec<OwnedObjectPath> = nm.call("GetDevices", &()).map_err(err)?;
    let mut best: Option<(u8, Target)> = None;
    for dev in devices {
        let d = proxy(conn, dev.as_str(), DEVICE_IFACE)?;
        if d.get_property::<u32>("DeviceType").ok() != Some(NM_DEVICE_TYPE_WIFI) {
            continue;
        }
        let w = proxy(conn, dev.as_str(), WIRELESS_IFACE)?;
        let aps: Vec<OwnedObjectPath> = w.get_property("AccessPoints").map_err(err)?;
        for ap in aps {
            let a = proxy(conn, ap.as_str(), AP_IFACE)?;
            // An AP can vanish between listing and reading it; skip it.
            let Ok(ap_ssid) = a.get_property::<Vec<u8>>("Ssid") else {
                continue;
            };
            if ap_ssid != ssid.as_bytes() {
                continue;
            }
            let strength: u8 = a.get_property("Strength").unwrap_or(0);
            if best.as_ref().is_some_and(|(s, _)| *s >= strength) {
                continue;
            }
            let target = Target {
                device: dev.clone(),
                flags: a.get_property("Flags").unwrap_or(0),
                wpa_flags: a.get_property("WpaFlags").unwrap_or(0),
                rsn_flags: a.get_property("RsnFlags").unwrap_or(0),
                ap,
            };
            best = Some((strength, target));
        }
    }
    best.map(|(_, t)| t)
        .ok_or_else(|| format!("'{}' is not in range", ssid))
}

/// The `802-11-wireless-security` secrets for a new profile, mirroring what
/// `nmcli device wifi connect` sends. Key management is left out so
/// NetworkManager completes it from the access point (wpa-psk or sae).
fn security_for(
    t: &Target,
    password: &str,
) -> Result<Option<HashMap<&'static str, Value<'static>>>, String> {
    let sec = t.wpa_flags | t.rsn_flags;
    if sec & (AP_SEC_KEY_MGMT_PSK | AP_SEC_KEY_MGMT_SAE) != 0 {
        return Ok(Some(HashMap::from([(
            "psk",
            Value::from(password.to_string()),
        )])));
    }
    if sec & AP_SEC_KEY_MGMT_802_1X != 0 {
        return Err(
            "this is an enterprise (802.1X) network; it needs more than a password".to_string(),
        );
    }
    if sec == 0 && t.flags & AP_FLAGS_PRIVACY != 0 {
        // WEP: a 5/13-character or 10/26-hex-digit key, else a passphrase.
        let hex = password.chars().all(|c| c.is_ascii_hexdigit());
        let is_key = matches!(password.len(), 5 | 13) || (hex && matches!(password.len(), 10 | 26));
        return Ok(Some(HashMap::from([
            ("key-mgmt", Value::from("none")),
            ("wep-key0", Value::from(password.to_string())),
            (
                "wep-key-type",
                Value::from(if is_key { 1u32 } else { 2u32 }),
            ),
        ])));
    }
    // Open (or OWE) network: there is no password to send.
    Ok(None)
}

fn str_of(v: Option<&OwnedValue>) -> Option<String> {
    v.and_then(|v| String::try_from(v.clone()).ok())
}

fn bytes_of(v: Option<&OwnedValue>) -> Option<Vec<u8>> {
    v.and_then(|v| Vec::<u8>::try_from(v.clone()).ok())
}

/// Saved connection profiles with their settings (secrets not included).
fn saved_connections(conn: &Connection) -> Result<Vec<(OwnedObjectPath, Settings)>, String> {
    let settings = proxy(conn, SETTINGS_PATH, SETTINGS_IFACE)?;
    let paths: Vec<OwnedObjectPath> = settings.call("ListConnections", &()).map_err(err)?;
    Ok(paths
        .into_iter()
        .filter_map(|p| {
            let c = proxy(conn, p.as_str(), CONN_IFACE).ok()?;
            let s: Settings = c.call("GetSettings", &()).ok()?;
            Some((p, s))
        })
        .collect())
}

/// Store `password` as the profile's PSK (or WEP key). A no-op when it is
/// already the stored value, so re-saving the Advanced dialog with the
/// pre-filled password writes nothing.
fn store_password(
    conn: &Connection,
    path: &OwnedObjectPath,
    mut settings: Settings,
    password: &str,
) -> Result<(), String> {
    let c = proxy(conn, path.as_str(), CONN_IFACE)?;
    // GetSettings omits secrets and Update replaces the whole profile, so
    // fetch this setting's secrets and send them back along with the change.
    let secrets: Settings = c.call("GetSecrets", &(WSEC,)).unwrap_or_default();
    let wsec = settings.entry(WSEC.to_string()).or_default();
    if let Some(existing) = secrets.get(WSEC) {
        for (k, v) in existing {
            wsec.insert(k.clone(), v.clone());
        }
    }
    let key = if str_of(wsec.get("key-mgmt")).as_deref() == Some("none") {
        "wep-key0"
    } else {
        "psk"
    };
    if str_of(wsec.get(key)).as_deref() == Some(password) {
        return Ok(());
    }
    let value =
        OwnedValue::try_from(Value::from(password.to_string())).map_err(|e| e.to_string())?;
    wsec.insert(key.to_string(), value);
    c.call::<_, _, ()>("Update", &(settings,)).map_err(err)
}

/// Set the stored password of the saved Wi-Fi profile named `id` (the name
/// nmcli shows), without the password ever appearing in a process argument
/// list.
pub fn set_wifi_password(id: &str, password: &str) -> Result<(), String> {
    let conn = system_bus()?;
    let (path, settings) = saved_connections(&conn)?
        .into_iter()
        .find(|(_, s)| {
            let c = s.get("connection");
            str_of(c.and_then(|c| c.get("id"))).as_deref() == Some(id)
                && str_of(c.and_then(|c| c.get("type"))).as_deref() == Some("802-11-wireless")
        })
        .ok_or_else(|| format!("no saved Wi-Fi connection named '{}'", id))?;
    store_password(&conn, &path, settings, password)
}

/// Wait for an activation to finish. Ok once the connection is up.
fn wait_activated(conn: &Connection, active: &OwnedObjectPath) -> Result<(), String> {
    let deadline = Instant::now() + ACTIVATION_TIMEOUT;
    let a = proxy(conn, active.as_str(), ACTIVE_IFACE)?;
    loop {
        match a.get_property::<u32>("State") {
            Ok(NM_ACTIVE_CONNECTION_STATE_ACTIVATED) => return Ok(()),
            // The active-connection object disappears when activation fails.
            Ok(NM_ACTIVE_CONNECTION_STATE_DEACTIVATED) | Err(_) => {
                return Err("connection failed (wrong password?)".to_string());
            }
            Ok(_) if Instant::now() >= deadline => {
                return Err(format!("timed out after {}s", ACTIVATION_TIMEOUT.as_secs()));
            }
            Ok(_) => std::thread::sleep(Duration::from_millis(250)),
        }
    }
}

/// Connect to a visible network with a password: the D-Bus equivalent of
/// `nmcli --wait 15 device wifi connect SSID password PASS`.
///
/// A saved profile for the SSID is reused, with its password updated. If
/// there is none, a new profile is created, and deleted again if activation
/// fails, so a mistyped password does not leave a broken saved network
/// behind.
pub fn connect_with_password(ssid: &str, password: &str) -> Result<(), String> {
    let conn = system_bus()?;
    let target = find_access_point(&conn, ssid)?;
    let nm = proxy(&conn, NM_PATH, NM_IFACE)?;

    let existing = saved_connections(&conn)?.into_iter().find(|(_, s)| {
        bytes_of(s.get("802-11-wireless").and_then(|w| w.get("ssid"))).as_deref()
            == Some(ssid.as_bytes())
    });
    if let Some((path, settings)) = existing {
        store_password(&conn, &path, settings, password)?;
        let active: OwnedObjectPath = nm
            .call("ActivateConnection", &(&path, &target.device, &target.ap))
            .map_err(err)?;
        return wait_activated(&conn, &active);
    }

    let mut settings: HashMap<&str, HashMap<&str, Value>> = HashMap::from([
        (
            "connection",
            HashMap::from([("id", Value::from(ssid.to_string()))]),
        ),
        (
            "802-11-wireless",
            HashMap::from([("ssid", Value::from(ssid.as_bytes().to_vec()))]),
        ),
    ]);
    if let Some(sec) = security_for(&target, password)? {
        settings.insert(WSEC, sec);
    }
    let (path, active): (OwnedObjectPath, OwnedObjectPath) = nm
        .call(
            "AddAndActivateConnection",
            &(settings, &target.device, &target.ap),
        )
        .map_err(err)?;
    let result = wait_activated(&conn, &active);
    if result.is_err()
        && let Ok(c) = proxy(&conn, path.as_str(), CONN_IFACE)
    {
        let _ = c.call::<_, _, ()>("Delete", &());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(flags: u32, wpa: u32, rsn: u32) -> Target {
        Target {
            device: OwnedObjectPath::try_from("/d").unwrap(),
            ap: OwnedObjectPath::try_from("/a").unwrap(),
            flags,
            wpa_flags: wpa,
            rsn_flags: rsn,
        }
    }

    #[test]
    fn security_settings_follow_the_access_point() {
        // WPA2-PSK and WPA3-SAE: just the psk; NetworkManager picks key-mgmt.
        for t in [
            target(1, 0, AP_SEC_KEY_MGMT_PSK),
            target(1, 0, AP_SEC_KEY_MGMT_SAE),
        ] {
            let sec = security_for(&t, "hunter22").unwrap().unwrap();
            assert_eq!(sec.keys().collect::<Vec<_>>(), [&"psk"]);
        }
        // Enterprise is refused rather than half-configured.
        assert!(security_for(&target(1, 0, AP_SEC_KEY_MGMT_802_1X), "x").is_err());
        // Open network: nothing to send.
        assert!(security_for(&target(0, 0, 0), "x").unwrap().is_none());
        // WEP: 13 characters is a key (type 1); anything else a passphrase.
        let wep = security_for(&target(AP_FLAGS_PRIVACY, 0, 0), "abcdefghijklm")
            .unwrap()
            .unwrap();
        assert_eq!(wep["wep-key-type"], Value::from(1u32));
        let wep = security_for(&target(AP_FLAGS_PRIVACY, 0, 0), "a passphrase")
            .unwrap()
            .unwrap();
        assert_eq!(wep["wep-key-type"], Value::from(2u32));
    }

    // The tests below talk to the real NetworkManager, so they are ignored by
    // default. Run them by hand with:
    //   cargo test --bin auralink nm_dbus -- --ignored --test-threads=1
    // They never activate anything: the profile they create has autoconnect
    // off and an SSID that is not in range.

    const TEST_ID: &str = "auralink-selftest";

    fn nmcli(args: &[&str]) -> std::process::Output {
        std::process::Command::new("nmcli")
            .args(args)
            .output()
            .unwrap()
    }

    fn stored_psk() -> String {
        let out = nmcli(&[
            "-s",
            "-g",
            "802-11-wireless-security.psk",
            "connection",
            "show",
            "id",
            TEST_ID,
        ]);
        // -g escapes ':' and '\\' in values.
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .replace("\\:", ":")
            .replace("\\\\", "\\")
    }

    #[test]
    #[ignore = "creates and deletes a NetworkManager profile"]
    fn set_wifi_password_updates_the_stored_psk() {
        let _ = nmcli(&["connection", "delete", "id", TEST_ID]);
        // A throwaway password on argv is fine here; it guards nothing.
        let add = nmcli(&[
            "connection",
            "add",
            "type",
            "wifi",
            "con-name",
            TEST_ID,
            "ssid",
            "auralink-selftest-ssid",
            "connection.autoconnect",
            "no",
            "wifi-sec.key-mgmt",
            "wpa-psk",
            "wifi-sec.psk",
            "initial-pass",
        ]);
        assert!(
            add.status.success(),
            "{}",
            String::from_utf8_lossy(&add.stderr)
        );

        let result = set_wifi_password(TEST_ID, "changed pass: with 'quotes' & spaces");
        let after = stored_psk();
        // Unchanged value: must succeed without writing.
        let again = set_wifi_password(TEST_ID, "changed pass: with 'quotes' & spaces");
        let missing = set_wifi_password("auralink-no-such-profile", "whatever");
        let _ = nmcli(&["connection", "delete", "id", TEST_ID]);

        assert_eq!(result, Ok(()));
        assert_eq!(after, "changed pass: with 'quotes' & spaces");
        assert_eq!(again, Ok(()));
        assert!(missing.unwrap_err().contains("no saved Wi-Fi connection"));
    }

    #[test]
    #[ignore = "reads live NetworkManager state"]
    fn connect_refuses_a_network_that_is_not_in_range() {
        let before = nmcli(&["-t", "-f", "NAME", "connection", "show"]).stdout;
        let res = connect_with_password("auralink-no-such-ssid-7f3a", "hunter22");
        assert!(res.unwrap_err().contains("not in range"));
        assert_eq!(
            nmcli(&["-t", "-f", "NAME", "connection", "show"]).stdout,
            before,
            "no profile created"
        );
    }
}
