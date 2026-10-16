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
    let path = dir.join(format!("prism-device-report-{safe_id}-{stamp}.json"));
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
    fn print_nightsword_facts() {
        let f = super::facts(0x1b1c, 0x1b5c).unwrap();
        println!("{}", serde_json::to_string_pretty(&f).unwrap());
    }
}
