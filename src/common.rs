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
        let card_bg = Color::from_rgb_u8(lift(bg.red()), lift(bg.green()), lift(bg.blue()));
        // Accent and danger colour text and thin outlines, so they need text
        // contrast against the cards they sit on. pywal's color1 is often a
        // dim shade of the wallpaper (2.2:1 on one real palette), which made
        // the title, buttons and switches hard to see.
        let readable = |c: Color| with_contrast(c, card_bg, 4.5);
        Some(Theme {
            // Translucent background for the glass effect.
            background: Color::from_argb_u8(0x88, bg.red(), bg.green(), bg.blue()),
            card_bg,
            accent: readable(pick(&wal.colors, "color1", d.accent)),
            foreground: fg,
            secondary_fg: Color::from_argb_u8(180, fg.red(), fg.green(), fg.blue()),
            separator: Color::from_argb_u8(60, fg.red(), fg.green(), fg.blue()),
            danger: readable(pick(&wal.colors, "color5", d.danger)),
            upload: with_contrast(pick(&wal.colors, "color2", d.upload), card_bg, 3.0),
        })
    }
}

/// WCAG relative luminance of an opaque colour.
fn luminance(c: Color) -> f32 {
    let lin = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.red()) + 0.7152 * lin(c.green()) + 0.0722 * lin(c.blue())
}

/// WCAG contrast ratio between two opaque colours (1 to 21).
fn contrast(a: Color, b: Color) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// `c`, brightened (on a dark background) or darkened (on a light one) just
/// enough to reach `min` contrast against `bg`. The hue is kept; saturation
/// gives way only once brightness is exhausted.
fn with_contrast(c: Color, bg: Color, min: f32) -> Color {
    let dark_bg = luminance(bg) < 0.5;
    let hsv = c.to_hsva();
    let (mut s, mut v) = (hsv.saturation, hsv.value);
    let mut out = c;
    for _ in 0..100 {
        if contrast(out, bg) >= min {
            break;
        }
        if dark_bg {
            if v < 1.0 {
                v = (v + 0.02).min(1.0);
            } else if s > 0.0 {
                s = (s - 0.04).max(0.0);
            } else {
                break;
            }
        } else if v > 0.0 {
            v = (v - 0.02).max(0.0);
        } else {
            break;
        }
        out = Color::from_hsva(hsv.hue, s, v, 1.0);
    }
    out
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
                        "colors":{"color1":"#33ccff"}}"##;
        let t = Theme::from_pywal_json(json).unwrap();
        assert_eq!(t.background, Color::from_argb_u8(0x88, 0, 0, 0));
        assert_eq!(t.card_bg, Color::from_rgb_u8(15, 15, 15));
        // Readable already, so used as is.
        assert_eq!(t.accent, Color::from_rgb_u8(0x33, 0xcc, 0xff));
        // color5 missing -> default danger.
        assert_eq!(t.danger, Theme::default_dark().danger);
        assert!(Theme::from_pywal_json("not json").is_none());
    }

    #[test]
    fn dim_pywal_accents_are_brightened_to_text_contrast() {
        // A real palette: color1 was 2.2:1 against the background.
        let json = r##"{"special":{"background":"#150a09","foreground":"#c4c1c1"},
                        "colors":{"color1":"#6E3941","color2":"#704E42","color5":"#D72C17"}}"##;
        let t = Theme::from_pywal_json(json).unwrap();
        assert!(contrast(t.accent, t.card_bg) >= 4.5);
        assert!(contrast(t.danger, t.card_bg) >= 4.5);
        assert!(contrast(t.upload, t.card_bg) >= 3.0);
        // Still the palette's hue, not a different colour.
        let (before, after) = (parse_hex("#6E3941").unwrap().to_hsva(), t.accent.to_hsva());
        assert!(
            (before.hue - after.hue).abs() < 2.0,
            "{} -> {}",
            before.hue,
            after.hue
        );
        assert!(after.value > before.value);
    }

    #[test]
    fn readable_colours_are_left_alone() {
        let bg = Color::from_rgb_u8(0x18, 0x18, 0x1b);
        let cyan = Color::from_rgb_u8(0x00, 0xf0, 0xff);
        assert_eq!(with_contrast(cyan, bg, 4.5), cyan);
        // On a light background a pale colour is darkened instead.
        let white_bg = Color::from_rgb_u8(0xff, 0xff, 0xff);
        let pale = Color::from_rgb_u8(0xff, 0xcc, 0xcc);
        assert!(contrast(with_contrast(pale, white_bg, 4.5), white_bg) >= 4.5);
    }

    #[test]
    fn format_speed_units() {
        assert_eq!(format_speed(512.0), "512 B/s");
        assert_eq!(format_speed(2048.0), "2.0 KB/s");
        assert_eq!(format_speed(3.5 * 1024.0 * 1024.0), "3.5 MB/s");
    }
}
