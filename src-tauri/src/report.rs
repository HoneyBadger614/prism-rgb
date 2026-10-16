//! "Help add this device": a report someone can attach to a GitHub issue so
//! Prism can support their device. Everything here only reads what Linux
//! already knows about the device; nothing is sent to it. The screen shows
//! the user every field before anything is saved, and Prism never sends the
//! report anywhere itself.

use hidapi::HidApi;
use serde::Serialize;

/// One HID interface: how the device presents itself.
#[derive(Debug, Clone, Serialize)]
pub struct Interface {
    pub number: i32,
    pub usage_page: String,
    pub usage: String,
    /// The device's own description of its reports (HID report descriptor),
    /// as hex. It describes the message formats, not anything about you.
    pub descriptor: Option<String>,
}

/// One Linux input device (keys and buttons) belonging to it.
#[derive(Debug, Clone, Serialize)]
pub struct InputNode {
    pub name: String,
    /// Keys and buttons it says it *can* send (not anything pressed).
    pub can_send: Vec<String>,
    pub moves_pointer: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Facts {
    pub prism_version: &'static str,
    pub linux_version: String,
    pub usb_id: String,
    pub product: String,
    pub manufacturer: String,
    /// Always removed; only whether the device has one.
    pub serial_number: &'static str,
    /// What Prism already knows about it.
    pub prism_knows: String,
    /// Which Corsair protocol it looks like, from its interfaces.
    pub looks_like: String,
    pub interfaces: Vec<Interface>,
    pub input_devices: Vec<InputNode>,
}

/// The device's facts. Read-only: nothing is sent to it.
pub fn facts(vid: u16, pid: u16) -> Result<Facts, String> {
    let api = HidApi::new().map_err(|e| format!("Couldn't start the USB device library: {e}"))?;
    let infos: Vec<_> = api.device_list().filter(|d| d.vendor_id() == vid && d.product_id() == pid).collect();
    let first = infos.first().ok_or("That device isn't connected")?;
    let product = first.product_string().unwrap_or_default().trim().to_string();
    let manufacturer = first.manufacturer_string().unwrap_or_default().trim().to_string();
    let has_serial = infos.iter().any(|d| d.serial_number().is_some_and(|s| !s.trim().is_empty()));

    let interfaces = hid_interfaces(&infos);

    let pages: Vec<&str> = interfaces.iter().map(|i| i.usage_page.as_str()).collect();
    let looks_like = if vid != prism_core::CORSAIR_VID {
        "not a Corsair device".to_string()
    } else if prism_core::lighting_node::model(pid).is_some() {
        "Lighting Node family".into()
    } else if pages.contains(&"0xff42") {
        "Corsair Bragi protocol (newer devices)".into()
    } else if pages.contains(&"0xffc2") {
        "Corsair's older protocol (like the Nightsword)".into()
    } else {
        "unknown".into()
    };

    let prism_knows = match prism_core::lookup_model(pid).filter(|_| vid == prism_core::CORSAIR_VID) {
        Some(m) if m.tested => format!("{}: supported and tested", m.name),
        Some(m) => format!("{}: supported but not yet tested", m.name),
        None => "nothing yet".into(),
    };

    let input_devices = prism_input::corsair_devices()
        .into_iter()
        .filter(|d| d.product == pid && vid == prism_core::CORSAIR_VID)
        .map(|d| {
            let mut can_send: Vec<String> = d.key_codes().into_iter().map(prism_input::raw_name).collect();
            can_send.truncate(800);
            InputNode { name: d.name.clone(), can_send, moves_pointer: d.has_movement() }
        })
        .collect();

    Ok(Facts {
        prism_version: env!("CARGO_PKG_VERSION"),
        linux_version: std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default().trim().to_string(),
        usb_id: format!("{vid:04x}:{pid:04x}"),
        product,
        manufacturer,
        serial_number: if has_serial { "removed (the device has one)" } else { "none" },
        prism_knows,
        looks_like,
        interfaces,
        input_devices,
    })
}

/// A device's HID interfaces, with each one's report descriptor read from
/// /sys (Linux keeps a copy; the device isn't asked).
fn hid_interfaces(infos: &[&hidapi::DeviceInfo]) -> Vec<Interface> {
    let mut interfaces: Vec<Interface> = infos
        .iter()
        .map(|d| {
            let path = d.path().to_string_lossy().into_owned();
            let node = path.rsplit('/').next().unwrap_or_default().to_string();
            let descriptor = node
                .starts_with("hidraw")
                .then(|| std::fs::read(format!("/sys/class/hidraw/{node}/device/report_descriptor")).ok())
                .flatten()
                .filter(|b| b.len() <= 4096)
                .map(|b| b.iter().map(|x| format!("{x:02x}")).collect::<String>());
            Interface { number: d.interface_number(), usage_page: format!("{:#06x}", d.usage_page()), usage: format!("{:#06x}", d.usage()), descriptor }
        })
        .collect();
    interfaces.sort_by(|a, b| (a.number, &a.usage_page, &a.usage).cmp(&(b.number, &b.usage_page, &b.usage)));
    interfaces.dedup_by(|a, b| a.number == b.number && a.usage_page == b.usage_page && a.usage == b.usage);
    interfaces
}

/* ---------- other RGB hardware ---------- */

/// USB makers whose devices often have lighting (Corsair has its own report).
const RGB_MAKERS: &[(u16, &str)] = &[
    (0x0b05, "ASUS"),
    (0x1462, "MSI"),
    (0x048d, "ITE (Gigabyte RGB Fusion and others)"),
    (0x1e71, "NZXT"),
    (0x1532, "Razer"),
    (0x2516, "Cooler Master"),
    (0x264a, "Thermaltake"),
    (0x0cf2, "Lian Li (ENE)"),
    (0x1038, "SteelSeries"),
    (0x046d, "Logitech"),
    (0x0951, "HyperX (Kingston)"),
    (0x03f0, "HyperX (HP)"),
    (0x1e7d, "Roccat"),
    (0x3842, "EVGA"),
    (0x26ce, "ASRock"),
    (0x04d9, "Holtek (many RGB keyboards and mice)"),
    (0x258a, "Sinowealth (many RGB keyboards and mice)"),
    (0x1b80, "Gainward / Palit"),
    (0x2433, "ASETEK (some coolers)"),
];

#[derive(Debug, Clone, Serialize)]
pub struct UsbItem {
    pub usb_id: String,
    pub maker_guess: &'static str,
    pub product: String,
    pub manufacturer: String,
    pub serial_number: &'static str,
    pub interfaces: Vec<Interface>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GpuItem {
    /// The card's PCI IDs: chip maker, chip, board maker, board model.
    pub pci_id: String,
    pub subsystem_id: String,
    pub prism_supports: bool,
    /// Names of the card's I2C buses (where lighting chips usually sit).
    pub i2c_buses: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Board {
    pub maker: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OtherHardware {
    pub prism_version: &'static str,
    pub linux_version: String,
    pub usb_devices: Vec<UsbItem>,
    pub graphics_cards: Vec<GpuItem>,
    /// Maker and model only.
    pub motherboard: Option<Board>,
    /// Names of the motherboard's own I2C/SMBus adapters (nothing on them is probed).
    pub system_buses: Vec<String>,
}

fn sys(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default().trim().to_string()
}

/// Lighting-capable hardware that isn't a Corsair USB device. Read-only:
/// only what Linux already knows; nothing is sent to anything.
pub fn other_hardware() -> Result<OtherHardware, String> {
    let api = HidApi::new().map_err(|e| format!("Couldn't start the USB device library: {e}"))?;
    let mut ids: Vec<(u16, u16)> = api
        .device_list()
        .filter(|d| RGB_MAKERS.iter().any(|(v, _)| *v == d.vendor_id()))
        .map(|d| (d.vendor_id(), d.product_id()))
        .collect();
    ids.sort();
    ids.dedup();
    let usb_devices = ids
        .into_iter()
        .take(40)
        .map(|(vid, pid)| {
            let infos: Vec<_> = api.device_list().filter(|d| d.vendor_id() == vid && d.product_id() == pid).collect();
            let first = infos[0];
            UsbItem {
                usb_id: format!("{vid:04x}:{pid:04x}"),
                maker_guess: RGB_MAKERS.iter().find(|(v, _)| *v == vid).map_or("?", |m| m.1),
                product: first.product_string().unwrap_or_default().trim().to_string(),
                manufacturer: first.manufacturer_string().unwrap_or_default().trim().to_string(),
                serial_number: if infos.iter().any(|d| d.serial_number().is_some_and(|s| !s.trim().is_empty())) { "removed (it has one)" } else { "none" },
                interfaces: hid_interfaces(&infos),
            }
        })
        .collect();

    // Graphics cards: PCI display devices (class 0x03xxxx).
    let mut graphics_cards = Vec::new();
    for e in std::fs::read_dir("/sys/bus/pci/devices").map_err(|e| e.to_string())?.flatten() {
        let p = e.path();
        if !sys(&p.join("class")).starts_with("0x03") {
            continue;
        }
        let id = |f: &str| sys(&p.join(f)).trim_start_matches("0x").to_string();
        let (v, d, sv, sd) = (id("vendor"), id("device"), id("subsystem_vendor"), id("subsystem_device"));
        let hex = |s: &str| u16::from_str_radix(s, 16).ok();
        let prism_supports = prism_core::msi_gpu::SUPPORTED.iter().any(|m| (Some(m.0), Some(m.1), Some(m.2), Some(m.3)) == (hex(&v), hex(&d), hex(&sv), hex(&sd)));
        let mut i2c_buses: Vec<String> = std::fs::read_dir(&p)
            .map(|r| r.flatten().filter(|x| x.file_name().to_string_lossy().starts_with("i2c-")).map(|x| sys(&x.path().join("name"))).collect())
            .unwrap_or_default();
        i2c_buses.sort();
        graphics_cards.push(GpuItem { pci_id: format!("{v}:{d}"), subsystem_id: format!("{sv}:{sd}"), prism_supports, i2c_buses });
    }

    let dmi = std::path::Path::new("/sys/class/dmi/id");
    let (vendor, name) = (sys(&dmi.join("board_vendor")), sys(&dmi.join("board_name")));
    let motherboard = (!vendor.is_empty() || !name.is_empty()).then_some(Board { maker: vendor, model: name });

    // The motherboard's own buses (not graphics cards'), by name only.
    let mut system_buses: Vec<String> = std::fs::read_dir("/sys/bus/i2c/devices")
        .map(|r| r.flatten().filter(|x| x.file_name().to_string_lossy().starts_with("i2c-")).map(|x| sys(&x.path().join("name"))).filter(|n| !n.contains("NVIDIA") && !n.contains("AMDGPU") && !n.is_empty()).collect())
        .unwrap_or_default();
    system_buses.sort();
    system_buses.dedup();

    Ok(OtherHardware {
        prism_version: env!("CARGO_PKG_VERSION"),
        linux_version: sys(std::path::Path::new("/proc/sys/kernel/osrelease")),
        usb_devices,
        graphics_cards,
        motherboard,
        system_buses,
    })
}

/// Saves the report (as reviewed on screen) to the Downloads folder.
/// Returns where it went. The report must be JSON and reasonably small.
pub fn save(json: &str, usb_id: &str) -> Result<String, String> {
    if json.len() > 1_000_000 {
        return Err("The report is too big".into());
    }
    let value: serde_json::Value = serde_json::from_str(json).map_err(|_| "The report isn't valid".to_string())?;
    let safe_id: String = usb_id.chars().filter(|c| c.is_ascii_hexdigit()).take(8).collect();
    let dir = downloads_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let name = if usb_id == "other" { format!("prism-hardware-report-{stamp}.json") } else { format!("prism-device-report-{safe_id}-{stamp}.json") };
    let path = dir.join(name);
    std::fs::write(&path, serde_json::to_string_pretty(&value).unwrap_or_default()).map_err(|e| format!("Couldn't save: {e}"))?;
    Ok(path.to_string_lossy().into_owned())
}

fn downloads_dir() -> std::path::PathBuf {
    let from_xdg = std::process::Command::new("xdg-user-dir")
        .arg("DOWNLOAD")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| s.starts_with('/'));
    match from_xdg {
        Some(d) => d.into(),
        None => std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default().join("Downloads"),
    }
}

/// Opens GitHub's "new issue" page for Prism in the browser, with a title.
/// Nothing is attached or sent: the person attaches the file themselves.
pub fn open_issue_page(product: &str) -> Result<(), String> {
    let title: String = format!("Device report: {product}")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || " :-.()".contains(*c))
        .take(80)
        .collect();
    let encoded: String = title.chars().map(|c| if c == ' ' { "+".to_string() } else if c.is_ascii_alphanumeric() || "-.".contains(c) { c.to_string() } else { format!("%{:02X}", c as u8) }).collect();
    let url = format!("https://github.com/HoneyBadger614/prism-rgb/issues/new?title={encoded}");
    std::process::Command::new("xdg-open").arg(url).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    /// Prints a real device's facts (needs the Nightsword plugged in).
    #[test]
    #[ignore]
    fn print_other_hardware() {
        println!("{}", serde_json::to_string_pretty(&super::other_hardware().unwrap()).unwrap());
    }

    #[test]
    #[ignore]
    fn print_nightsword_facts() {
        let f = super::facts(0x1b1c, 0x1b5c).unwrap();
        println!("{}", serde_json::to_string_pretty(&f).unwrap());
    }
}
