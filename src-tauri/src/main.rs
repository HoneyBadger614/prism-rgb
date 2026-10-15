// Prism desktop app: the window and the bridge between the screen and prism-core.

use hidapi::HidApi;
use prism_core::{find_devices, DeviceKind, CORSAIR_VID};
use serde::Serialize;

/// What the screen needs to know about one device.
#[derive(Serialize)]
struct DeviceInfo {
    name: String,
    product: String,
    usb_id: String,
    kind: &'static str,
    recognized: bool,
    interfaces: usize,
}

/// Lists connected Corsair devices. Read-only: no device is opened.
#[tauri::command]
fn list_devices() -> Result<Vec<DeviceInfo>, String> {
    let api = HidApi::new().map_err(|e| format!("Could not start the USB device library: {e}"))?;
    Ok(find_devices(&api)
        .into_iter()
        .map(|d| DeviceInfo {
            name: d.display_name().to_string(),
            product: d.product.clone(),
            usb_id: format!("{:04x}:{:04x}", CORSAIR_VID, d.pid),
            kind: match d.model.map(|m| m.kind) {
                Some(DeviceKind::Keyboard) => "keyboard",
                Some(DeviceKind::Mouse) => "mouse",
                Some(DeviceKind::LightingController) => "lighting",
                None => "unknown",
            },
            recognized: d.is_supported(),
            interfaces: d.interfaces.len(),
        })
        .collect())
}

fn main() {
    // WebKitGTK's DMA-BUF renderer crashes on NVIDIA under Wayland
    // ("Error 71 (Protocol error)"). Turn it off unless the user chose otherwise.
    // Set before any threads start.
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![list_devices])
        .run(tauri::generate_context!())
        .expect("Prism failed to start");
}
