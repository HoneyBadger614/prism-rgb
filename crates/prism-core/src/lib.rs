//! Prism's device layer.
//!
//! For now this only *finds* Corsair devices. It never opens them or sends
//! them anything, so it is safe to run at any time.

use std::collections::BTreeMap;

use hidapi::HidApi;

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
}

/// Models Prism supports (or is working towards). Matched by USB product ID.
pub const KNOWN_MODELS: &[KnownModel] = &[
    KnownModel { pid: 0x1baf, name: "K65 RGB Mini", kind: DeviceKind::Keyboard },
    KnownModel { pid: 0x1b5c, name: "Nightsword RGB", kind: DeviceKind::Mouse },
    KnownModel { pid: 0x0c0b, name: "Lighting Node Pro", kind: DeviceKind::LightingController },
];

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
