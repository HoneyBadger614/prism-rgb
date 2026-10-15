// Prism desktop app: the window and the bridge between the screen and prism-core.

mod engine;

use engine::{Devices, Engine};
use hidapi::HidApi;
use prism_core::effects::{ChannelLayout, LightingStore, Surface};
use prism_core::k65_mini::{self, Key};
use prism_core::nightsword;
use prism_core::{find_devices, DeviceKind, CORSAIR_VID};
use serde::Serialize;
use tauri::{Manager, RunEvent, State};

/// What's plugged into the Lighting Node Pro.
/// Channel 1: three fans, 4 hub + 12 ring LEDs each. Channel 2: nothing (checked
/// with `prism-light identify 2`). This will move to a settings screen.
fn node_layout() -> Vec<ChannelLayout> {
    vec![
        ChannelLayout::ll_fans(3),
        ChannelLayout::ll_fans(0),
    ]
}

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

struct AppState {
    engine: Engine,
    layout: Vec<ChannelLayout>,
    fans: Surface,
    keyboard: Surface,
    keys: Vec<Key>,
    mouse: Surface,
}

#[derive(Serialize)]
struct LightingState {
    store: LightingStore,
    /// The Lighting Node Pro's fans.
    layout: Vec<ChannelLayout>,
    /// The K65's keys, in LED order.
    keys: Vec<Key>,
    /// The Nightsword's lighting zones, in LED order.
    mouse_zones: Vec<&'static str>,
}

#[tauri::command]
fn get_lighting(state: State<AppState>) -> LightingState {
    LightingState {
        store: state.engine.config(),
        layout: state.layout.clone(),
        keys: state.keys.clone(),
        mouse_zones: nightsword::ZONES.iter().map(|z| z.0).collect(),
    }
}

/// Applies new lighting settings (all profiles). Returns them with anything
/// out of range fixed.
#[tauri::command]
fn set_lighting(state: State<AppState>, store: LightingStore) -> LightingStore {
    let store = store.sanitized(&state.fans, &state.keyboard, &state.mouse);
    state.engine.set_config(store.clone());
    store
}

fn main() {
    // WebKitGTK's DMA-BUF renderer crashes on NVIDIA under Wayland
    // ("Error 71 (Protocol error)"). Turn it off unless the user chose otherwise.
    // Set before any threads start.
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    let app = tauri::Builder::default()
        .setup(|app| {
            // ~/.config/io.github.honeybadger.prism/lighting.json
            let save_to = app.path().app_config_dir().ok().map(|d| d.join("lighting.json"));
            let layout = node_layout();
            let keys = k65_mini::keys();
            let devices = Devices {
                fans: Surface::fans(&layout),
                keyboard: k65_mini::surface(&keys),
                keys: keys.clone(),
                mouse: nightsword::surface(),
            };
            let (fans, keyboard, mouse) = (devices.fans.clone(), devices.keyboard.clone(), devices.mouse.clone());
            let config = engine::load(&save_to, &devices);
            let engine = Engine::spawn(app.handle().clone(), config, devices, save_to);
            app.manage(AppState { engine, layout, fans, keyboard, keys, mouse });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![list_devices, get_lighting, set_lighting])
        .build(tauri::generate_context!())
        .expect("Prism failed to start");

    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            handle.state::<AppState>().engine.stop();
        }
    });
}
