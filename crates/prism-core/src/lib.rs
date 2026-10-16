//! Prism's device layer.
//!
//! [`find_devices`] only *finds* Corsair devices; it never opens them or sends
//! them anything. Each device module (e.g. [`lighting_node`]) handles talking
//! to one model.

use std::collections::BTreeMap;

use hidapi::HidApi;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub mod asus_aura;
pub mod bindings;
pub mod effects;
pub mod k65_mini;
pub mod kb_onboard;
pub mod lighting_node;
pub mod mouse_onboard;
pub mod msi_gpu;
pub mod nightsword;

/// A color, 0–255 per part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Reads "#ff2d95" or "ff2d95".
    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.strip_prefix('#').unwrap_or(s);
        if s.len() != 6 || !s.is_ascii() {
            return None;
        }
        let part = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
        Some(Self::new(part(0)?, part(2)?, part(4)?))
    }

    /// Converts a screen color to what an LED needs to look the same.
    ///
    /// Screen colors are gamma-encoded (small numbers look darker than they
    /// are), but LEDs are linear: a value of 45 really is 18% power. Without
    /// this, small amounts of a color wash out the main one (hot pink looks
    /// pastel).
    pub fn to_led(self) -> Rgb {
        const GAMMA: f32 = 2.2;
        let f = |v: u8| ((v as f32 / 255.0).powf(GAMMA) * 255.0).round() as u8;
        Rgb::new(f(self.r), f(self.g), f(self.b))
    }

    /// Blends towards `other`; `t` = 0 gives self, 1 gives other.
    pub fn mix(self, other: Rgb, t: f32) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        let f = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Rgb::new(f(self.r, other.r), f(self.g, other.g), f(self.b, other.b))
    }

    /// From hue (0–360), saturation and value (0–1).
    pub fn from_hsv(h: f32, s: f32, v: f32) -> Rgb {
        let f = |n: f32| {
            let k = (n + h / 60.0).rem_euclid(6.0);
            let x = v - v * s * k.min(4.0 - k).clamp(0.0, 1.0);
            (x * 255.0).round() as u8
        };
        Rgb::new(f(5.0), f(3.0), f(1.0))
    }

    /// Dims towards black; `level` = 1 keeps the color, 0 is off.
    pub fn scale(self, level: f32) -> Rgb {
        Rgb::new(0, 0, 0).mix(self, level)
    }

    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

// Colors are stored and sent to the screen as "#rrggbb".
impl Serialize for Rgb {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Rgb::from_hex(&s).ok_or_else(|| serde::de::Error::custom(format!("'{s}' is not a color like #ff2d95")))
    }
}

/// Corsair's USB vendor ID. Every Corsair device reports this number.
pub const CORSAIR_VID: u16 = 0x1b1c;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Keyboard,
    Mouse,
    LightingController,
}

/// A Corsair model that Prism knows about.
#[derive(Debug)]
pub struct KnownModel {
    pub pid: u16,
    pub name: &'static str,
    pub kind: DeviceKind,
    /// Checked on a real device. Others follow other projects' notes and
    /// are shown as experimental.
    pub tested: bool,
}

/// Models Prism supports (or is working towards). Matched by USB product ID.
pub const KNOWN_MODELS: &[KnownModel] = &[
    KnownModel { pid: 0x1baf, name: "K65 RGB Mini", kind: DeviceKind::Keyboard, tested: true },
    KnownModel { pid: 0x1b5c, name: "Nightsword RGB", kind: DeviceKind::Mouse, tested: true },
    KnownModel { pid: 0x0c0b, name: "Lighting Node Pro", kind: DeviceKind::LightingController, tested: true },
    // Same protocol as the Lighting Node Pro (see lighting_node::MODELS).
    KnownModel { pid: 0x0c1a, name: "Lighting Node Core", kind: DeviceKind::LightingController, tested: false },
    KnownModel { pid: 0x0c10, name: "Commander Pro", kind: DeviceKind::LightingController, tested: false },
    KnownModel { pid: 0x0c1e, name: "LS100 Lighting Kit", kind: DeviceKind::LightingController, tested: false },
    KnownModel { pid: 0x0c23, name: "LT100 Smart Lighting Towers", kind: DeviceKind::LightingController, tested: false },
    KnownModel { pid: 0x1d00, name: "Obsidian 1000D", kind: DeviceKind::LightingController, tested: false },
    KnownModel { pid: 0x1d04, name: "SPEC-OMEGA RGB", kind: DeviceKind::LightingController, tested: false },
];

/// Whether the Prism app is running. The test tools refuse to talk to devices
/// while it is: two programs talking to one device get each other's answers.
pub fn prism_app_running() -> bool {
    let Ok(procs) = std::fs::read_dir("/proc") else { return false };
    procs.flatten().any(|p| {
        std::fs::read_to_string(p.path().join("comm")).is_ok_and(|c| c.trim() == "prism-rgb")
    })
}

pub fn lookup_model(pid: u16) -> Option<&'static KnownModel> {
    KNOWN_MODELS.iter().find(|m| m.pid == pid)
}

/// One USB interface of a device. A single keyboard usually shows up as
/// several of these (normal typing, media keys, a channel for lighting...).
#[derive(Debug, Clone)]
pub struct Interface {
    pub number: i32,
    pub usage_page: u16,
    pub usage: u16,
    pub path: String,
}

/// A physical Corsair device, with all of its interfaces grouped together.
#[derive(Debug, Clone)]
pub struct FoundDevice {
    pub pid: u16,
    pub model: Option<&'static KnownModel>,
    /// The name the device reports about itself.
    pub product: String,
    pub serial: String,
    pub interfaces: Vec<Interface>,
}

impl FoundDevice {
    pub fn display_name(&self) -> &str {
        match self.model {
            Some(m) => m.name,
            None if !self.product.is_empty() => &self.product,
            None => "Unknown Corsair device",
        }
    }

    pub fn is_supported(&self) -> bool {
        self.model.is_some()
    }
}

/// Lists every connected Corsair device. Read-only: nothing is opened.
pub fn find_devices(api: &HidApi) -> Vec<FoundDevice> {
    let mut grouped: BTreeMap<(u16, String), FoundDevice> = BTreeMap::new();

    for info in api.device_list().filter(|d| d.vendor_id() == CORSAIR_VID) {
        let pid = info.product_id();
        let serial = info.serial_number().unwrap_or_default().to_string();

        let dev = grouped.entry((pid, serial.clone())).or_insert_with(|| FoundDevice {
            pid,
            model: lookup_model(pid),
            product: info.product_string().unwrap_or_default().trim().to_string(),
            serial,
            interfaces: Vec::new(),
        });

        dev.interfaces.push(Interface {
            number: info.interface_number(),
            usage_page: info.usage_page(),
            usage: info.usage(),
            path: info.path().to_string_lossy().into_owned(),
        });
    }

    let mut devices: Vec<FoundDevice> = grouped.into_values().collect();
    for d in &mut devices {
        d.interfaces.sort_by_key(|i| (i.number, i.usage_page, i.usage));
    }
    // Keyboards, then mice, then lighting, then anything unrecognized.
    devices.sort_by_key(|d| (d.model.map_or(3, |m| m.kind as u8), d.pid));
    devices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_the_three_target_devices() {
        assert_eq!(lookup_model(0x1baf).unwrap().kind, DeviceKind::Keyboard);
        assert_eq!(lookup_model(0x1b5c).unwrap().kind, DeviceKind::Mouse);
        assert_eq!(lookup_model(0x0c0b).unwrap().kind, DeviceKind::LightingController);
        assert!(lookup_model(0x0000).is_none());
    }

    #[test]
    fn lighting_controllers_match_the_driver() {
        for m in lighting_node::MODELS {
            let k = lookup_model(m.0).expect("listed");
            assert_eq!((k.name, k.kind, k.tested), (m.1, DeviceKind::LightingController, m.3));
        }
    }

    #[test]
    fn parses_hex_colors() {
        assert_eq!(Rgb::from_hex("#FF2D95"), Some(Rgb::new(0xff, 0x2d, 0x95)));
        assert_eq!(Rgb::from_hex("7c3aed"), Some(Rgb::new(0x7c, 0x3a, 0xed)));
        assert_eq!(Rgb::from_hex("#fff"), None);
        assert_eq!(Rgb::from_hex("zzzzzz"), None);
        assert_eq!(Rgb::from_hex("ééé"), None);
    }

    #[test]
    fn mixes_colors() {
        let a = Rgb::new(0, 0, 0);
        let b = Rgb::new(200, 100, 50);
        assert_eq!(a.mix(b, 0.0), a);
        assert_eq!(a.mix(b, 1.0), b);
        assert_eq!(a.mix(b, 0.5), Rgb::new(100, 50, 25));
    }

    #[test]
    fn hsv_primaries() {
        assert_eq!(Rgb::from_hsv(0.0, 1.0, 1.0), Rgb::new(255, 0, 0));
        assert_eq!(Rgb::from_hsv(120.0, 1.0, 1.0), Rgb::new(0, 255, 0));
        assert_eq!(Rgb::from_hsv(240.0, 1.0, 1.0), Rgb::new(0, 0, 255));
        assert_eq!(Rgb::from_hsv(300.0, 0.0, 1.0), Rgb::new(255, 255, 255));
        assert_eq!(Rgb::from_hsv(60.0, 1.0, 0.0), Rgb::new(0, 0, 0));
    }

    #[test]
    fn led_correction_keeps_ends_and_darkens_small_values() {
        assert_eq!(Rgb::new(0, 255, 0).to_led(), Rgb::new(0, 255, 0));
        let pink = Rgb::from_hex("ff2d95").unwrap().to_led();
        assert_eq!(pink.r, 255);
        assert!(pink.g < 10, "green should nearly vanish, got {}", pink.g);
        assert!(pink.b < 0x95);
    }

    #[test]
    fn unknown_device_falls_back_to_reported_name() {
        let d = FoundDevice {
            pid: 0x1234,
            model: None,
            product: "Mystery Thing".into(),
            serial: String::new(),
            interfaces: vec![],
        };
        assert_eq!(d.display_name(), "Mystery Thing");
        assert!(!d.is_supported());
    }
}
