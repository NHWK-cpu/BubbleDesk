//! Konfigurasi aplikasi (spec §15).
//!
//! Semua key opsional. Berkas yang tidak ada atau tidak dapat di-parse
//! menghasilkan nilai default disertai peringatan di log, bukan crash.

use serde::Deserialize;
use std::path::Path;

/// Mode pembukaan item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClickMode {
    Single,
    Double,
}

/// Sumber latar belakang jendela.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Background {
    Wallpaper,
    Gradient,
    Solid,
}

/// Konfigurasi lengkap. Key yang tidak ada memakai nilai default bagian terkait.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: General,
    pub sources: Sources,
    pub bubbles: Bubbles,
    pub physics: Physics,
    pub render: Render,
    pub power: Power,
    pub debug: Debug,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct General {
    pub start_with_windows: bool,
    pub monitor: String,
    pub click_mode: ClickMode,
    pub hide_native_icons: bool,
    pub reduce_motion: bool,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Sources {
    pub include: Vec<String>,
    pub ignore_patterns: Vec<String>,
    pub ignore_hidden: bool,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Bubbles {
    pub r_min: f64,
    pub r_max: f64,
    pub score_half_life_days: f64,
    pub score_cap: f64,
    pub min_hit_radius: f64,
    pub label_always_min_radius: f64,
    pub gap: f64,
    pub crowd_budget: f64,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Physics {
    pub damping: f64,
    pub drift: f64,
    pub restitution_wall: f64,
    pub restitution_body: f64,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Render {
    pub background: Background,
    pub background_dim: f64,
    pub background_blur: u32,
    pub solid_color: String,
    pub fps_cap: u32,
    pub fps_cap_on_battery: u32,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Power {
    pub pause_on_fullscreen: bool,
    pub pause_on_lock: bool,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Debug {
    pub overlay: bool,
    pub log_level: String,
}

impl Config {
    /// Memuat `config.toml` dari `dir`.
    pub fn load(dir: &Path) -> Self {
        let path = dir.join("config.toml");
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(e) => {
                log_warn!("tidak dapat membaca {}: {e}", path.display());
                return Self::default();
            }
        };
        match toml::from_str(&text) {
            Ok(config) => config,
            Err(e) => {
                log_warn!(
                    "{} tidak valid, memakai konfigurasi default: {e}",
                    path.display()
                );
                Self::default()
            }
        }
    }
}

impl Default for General {
    fn default() -> Self {
        Self {
            start_with_windows: false,
            monitor: "primary".to_owned(),
            // AGENTS §2: klik ganda membuka item.
            click_mode: ClickMode::Double,
            hide_native_icons: true,
            reduce_motion: false,
        }
    }
}

impl Default for Sources {
    fn default() -> Self {
        Self {
            include: vec!["desktop".to_owned(), "public_desktop".to_owned()],
            ignore_patterns: vec![
                "desktop.ini".to_owned(),
                "Thumbs.db".to_owned(),
                "~$*".to_owned(),
            ],
            ignore_hidden: true,
        }
    }
}

impl Default for Bubbles {
    fn default() -> Self {
        Self {
            r_min: 28.0,
            r_max: 110.0,
            score_half_life_days: 30.0,
            score_cap: 40.0,
            min_hit_radius: 20.0,
            label_always_min_radius: 60.0,
            gap: 6.0,
            crowd_budget: 0.55,
        }
    }
}

impl Default for Physics {
    fn default() -> Self {
        Self {
            damping: 3.0,
            drift: 0.5,
            restitution_wall: 0.3,
            restitution_body: 0.2,
        }
    }
}

impl Default for Render {
    fn default() -> Self {
        Self {
            background: Background::Wallpaper,
            background_dim: 0.35,
            background_blur: 6,
            solid_color: "#101820".to_owned(),
            fps_cap: 60,
            fps_cap_on_battery: 30,
        }
    }
}

impl Default for Power {
    fn default() -> Self {
        Self {
            pause_on_fullscreen: true,
            pause_on_lock: true,
        }
    }
}

impl Default for Debug {
    fn default() -> Self {
        Self {
            overlay: false,
            log_level: "info".to_owned(),
        }
    }
}

impl Render {
    /// Warna solid `solid_color` sebagai `COLORREF` (0x00BBGGRR), dengan fallback
    /// ke nilai default bila formatnya tidak dikenali.
    pub fn solid_color_ref(&self) -> u32 {
        parse_hex_color(&self.solid_color)
            .or_else(|| parse_hex_color(DEFAULT_SOLID_COLOR))
            .unwrap_or(0)
    }
}

/// Warna solid default bila `solid_color` tidak valid.
const DEFAULT_SOLID_COLOR: &str = "#101820";

fn parse_hex_color(value: &str) -> Option<u32> {
    let hex = value.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let rgb = u32::from_str_radix(hex, 16).ok()?;
    let r = (rgb >> 16) & 0xFF;
    let g = (rgb >> 8) & 0xFF;
    let b = rgb & 0xFF;
    Some(r | (g << 8) | (b << 16))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_from(text: &str) -> Config {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("config.toml"), text).expect("tulis config");
        Config::load(dir.path())
    }

    #[test]
    fn defaults_match_spec() {
        let c = Config::default();
        assert_eq!(c.general.click_mode, ClickMode::Double);
        assert!(c.general.hide_native_icons);
        assert_eq!(c.bubbles.r_min, 28.0);
        assert_eq!(c.bubbles.r_max, 110.0);
        assert_eq!(c.bubbles.crowd_budget, 0.55);
        assert_eq!(c.render.background, Background::Wallpaper);
        assert_eq!(c.render.fps_cap, 60);
        assert_eq!(c.physics.damping, 3.0);
    }

    #[test]
    fn missing_file_uses_default() {
        let dir = tempfile::tempdir().expect("tempdir");
        let c = Config::load(dir.path());
        assert_eq!(c.bubbles.r_max, 110.0);
    }

    #[test]
    fn invalid_toml_falls_back_to_default() {
        let c = load_from("ini = = bukan toml");
        assert_eq!(c.bubbles.r_max, 110.0);
    }

    #[test]
    fn unknown_enum_value_falls_back_to_default() {
        let c = load_from("[general]\nclick_mode = \"triple\"\n");
        assert_eq!(c.general.click_mode, ClickMode::Double);
    }

    #[test]
    fn partial_toml_keeps_other_defaults() {
        let c = load_from("[bubbles]\nr_max = 200.0\n");
        assert_eq!(c.bubbles.r_max, 200.0);
        assert_eq!(c.bubbles.r_min, 28.0);
    }

    #[test]
    fn solid_color_is_converted_to_colorref() {
        // #FF8000 -> R=FF G=80 B=00 -> COLORREF (0x00BBGGRR) = 0x0080FF
        let c = load_from("[render]\nsolid_color = \"#FF8000\"\n");
        assert_eq!(c.render.solid_color_ref(), 0x0080FF);
        // Nilai tidak valid jatuh ke default #101820 -> 0x201810.
        let c = load_from("[render]\nsolid_color = \"bukan-warna\"\n");
        assert_eq!(c.render.solid_color_ref(), 0x201810);
    }

    #[test]
    fn values_are_parsed() {
        let c = load_from("[general]\nclick_mode = \"single\"\n\n[render]\nfps_cap = 30\n");
        assert_eq!(c.general.click_mode, ClickMode::Single);
        assert_eq!(c.render.fps_cap, 30);
    }
}
