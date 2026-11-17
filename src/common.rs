//! Helpers shared by the `auralink` and `auralink-bt` binaries: the per-app
//! config file, desktop notifications and the pywal-driven colour theme.
//!
//! Each binary compiles this module separately (there is no library crate),
//! so items one of them does not use would otherwise warn.
#![allow(dead_code)]

use slint::Color;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

/// Settings persisted to `~/.config/auralink/<file>`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Default)]
pub struct AppConfig {
    pub pywal_enabled: bool,
}

impl AppConfig {
    fn path(file: &str) -> PathBuf {
        home().join(".config/auralink").join(file)
    }

    pub fn load(file: &str) -> Self {
        std::fs::read_to_string(Self::path(file))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, file: &str) {
        let path = Self::path(file);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(s) = serde_json::to_string(self) {
            let _ = std::fs::write(path, s);
        }
    }
}

/// Fire-and-forget desktop notification. `notify-send` is optional.
pub fn send_notification(app: &str, summary: &str, body: &str, icon: Option<&str>) {
    let mut cmd = std::process::Command::new("notify-send");
    cmd.args(["-a", app, "-t", "3500", "-u", "normal", summary, body]);
    if let Some(ic) = icon {
        cmd.args(["-i", ic]);
    }
    // Reap the child from a throwaway thread; a bare spawn() leaves a zombie
    // behind for every notification until the app exits.
    if let Ok(mut child) = cmd.spawn() {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

/// Parse `#rrggbb` or `#rrggbbaa`.
pub fn parse_hex(hex: &str) -> Option<Color> {
    let hex = hex.trim_start_matches('#');
    if !hex.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    match hex.len() {
        6 => Some(Color::from_rgb_u8(byte(0)?, byte(2)?, byte(4)?)),
        8 => Some(Color::from_argb_u8(byte(6)?, byte(0)?, byte(2)?, byte(4)?)),
        _ => None,
    }
}

/// The colours both windows theme themselves with. `auralink-bt` uses all
/// but `danger` and `upload`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub background: Color,
    pub card_bg: Color,
    pub accent: Color,
    pub foreground: Color,
    pub secondary_fg: Color,
    pub separator: Color,
    pub danger: Color,
    pub upload: Color,
}

impl Theme {
    /// The built-in dark theme; matches the defaults in the .slint globals.
    pub fn default_dark() -> Self {
        Theme {
            background: Color::from_argb_u8(0x88, 0x09, 0x09, 0x0b),
            card_bg: Color::from_rgb_u8(0x18, 0x18, 0x1b),
            accent: Color::from_rgb_u8(0x00, 0xf0, 0xff),
            foreground: Color::from_rgb_u8(0xf8, 0xfa, 0xfc),
            secondary_fg: Color::from_rgb_u8(0xa1, 0xa1, 0xaa),
            separator: Color::from_rgb_u8(0x27, 0x27, 0x2a),
            danger: Color::from_rgb_u8(0xff, 0x00, 0x55),
            upload: Color::from_rgb_u8(0xff, 0x00, 0x55),
        }
    }

    /// Build a theme from pywal's `colors.json`, or None if it is missing or
    /// unreadable.
    pub fn from_pywal() -> Option<Self> {
        let content = std::fs::read_to_string(pywal_colors_path()).ok()?;
        Self::from_pywal_json(&content)
    }

    fn from_pywal_json(content: &str) -> Option<Self> {
        #[derive(serde::Deserialize)]
        struct Wal {
            colors: std::collections::HashMap<String, String>,
            special: std::collections::HashMap<String, String>,
        }
        let wal: Wal = serde_json::from_str(content).ok()?;
        let d = Self::default_dark();
        let pick = |map: &std::collections::HashMap<String, String>, key: &str, fallback: Color| {
            map.get(key).and_then(|h| parse_hex(h)).unwrap_or(fallback)
        };

        let bg = pick(&wal.special, "background", d.background);
        let fg = pick(&wal.special, "foreground", d.foreground);
        let lift = |c: u8| c.saturating_add(15);
        Some(Theme {
            // Translucent background for the glass effect.
            background: Color::from_argb_u8(0x88, bg.red(), bg.green(), bg.blue()),
            card_bg: Color::from_rgb_u8(lift(bg.red()), lift(bg.green()), lift(bg.blue())),
            accent: pick(&wal.colors, "color1", d.accent),
            foreground: fg,
            secondary_fg: Color::from_argb_u8(180, fg.red(), fg.green(), fg.blue()),
            separator: Color::from_argb_u8(60, fg.red(), fg.green(), fg.blue()),
            danger: pick(&wal.colors, "color5", d.danger),
            upload: pick(&wal.colors, "color2", d.upload),
        })
    }
}

pub fn pywal_colors_path() -> PathBuf {
    home().join(".cache/wal/colors.json")
}

/// Re-theme live when pywal rewrites `colors.json`.
///
/// Polls the file's mtime every 2s while `enabled` is set and calls `apply`
/// with the new theme on each change. `apply` runs on the watcher thread, so
/// it must hop to the UI thread itself.
pub fn spawn_pywal_watch(enabled: Arc<AtomicBool>, apply: impl Fn(Theme) + Send + 'static) {
    std::thread::spawn(move || {
        let path = pywal_colors_path();
        let mut last_mod: Option<SystemTime> = None;
        loop {
            if enabled.load(Ordering::Relaxed) {
                if let Ok(mtime) = std::fs::metadata(&path).and_then(|m| m.modified())
                    && Some(mtime) != last_mod
                {
                    last_mod = Some(mtime);
                    if let Some(theme) = Theme::from_pywal() {
                        apply(theme);
                    }
                }
            } else {
                // Forget the mtime so re-enabling re-applies the current file.
                last_mod = None;
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

pub fn format_speed(bytes_per_sec: f64) -> String {
    const KIB: f64 = 1024.0;
    if bytes_per_sec >= KIB * KIB {
        format!("{:.1} MB/s", bytes_per_sec / (KIB * KIB))
    } else if bytes_per_sec >= KIB {
        format!("{:.1} KB/s", bytes_per_sec / KIB)
    } else {
        format!("{:.0} B/s", bytes_per_sec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_handles_rgb_rgba_and_garbage() {
        assert_eq!(
            parse_hex("#102030"),
            Some(Color::from_rgb_u8(0x10, 0x20, 0x30))
        );
        assert_eq!(
            parse_hex("10203040"),
            Some(Color::from_argb_u8(0x40, 0x10, 0x20, 0x30))
        );
        assert_eq!(parse_hex("#12345"), None);
        assert_eq!(parse_hex("#zz2030"), None);
        // Multi-byte input must not panic on a non-char-boundary slice.
        assert_eq!(parse_hex("#ééé"), None);
    }

    #[test]
    fn pywal_theme_uses_colors_and_falls_back() {
        let json = r##"{"special":{"background":"#000000","foreground":"#ffffff"},
                        "colors":{"color1":"#112233"}}"##;
        let t = Theme::from_pywal_json(json).unwrap();
        assert_eq!(t.background, Color::from_argb_u8(0x88, 0, 0, 0));
        assert_eq!(t.card_bg, Color::from_rgb_u8(15, 15, 15));
        assert_eq!(t.accent, Color::from_rgb_u8(0x11, 0x22, 0x33));
        // color5 missing -> default danger.
        assert_eq!(t.danger, Theme::default_dark().danger);
        assert!(Theme::from_pywal_json("not json").is_none());
    }

    #[test]
    fn format_speed_units() {
        assert_eq!(format_speed(512.0), "512 B/s");
        assert_eq!(format_speed(2048.0), "2.0 KB/s");
        assert_eq!(format_speed(3.5 * 1024.0 * 1024.0), "3.5 MB/s");
    }
}
