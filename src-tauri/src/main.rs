// Prism desktop app: the window and the bridge between the screen and prism-core.

mod apps;
mod autostart;
mod backup;
mod engine;
mod kb_save;
mod mouse_buttons;
mod mouse_save;
mod remap;
mod report;
mod tray;

use std::time::Duration;

use engine::{Devices, Engine};
use prism_core::bindings::{self, InputDevice};
use remap::Remap;
use hidapi::HidApi;
use prism_core::effects::{self, ChannelLayout, LightingStore, PieceKind, Surface};
use prism_core::lighting_node;
use prism_core::k65_mini::{self, Key};
use prism_core::asus_aura;
use prism_core::msi_gpu;
use prism_core::nightsword;
use prism_core::{find_devices, DeviceKind, CORSAIR_VID};
use serde::Serialize;
use tauri::{Manager, RunEvent, State};


/// What the screen needs to know about one device.
#[derive(Serialize)]
struct DeviceInfo {
    name: String,
    product: String,
    usb_id: String,
    /// How it's connected, for the subtitle ("USB 1b1c:0c0b", "I2C bus 3").
    bus: String,
    kind: &'static str,
    recognized: bool,
    /// Supported from other projects' notes, not yet checked on a real one.
    experimental: bool,
    /// Only lighting works (no bindings or onboard memory yet).
    lighting_only: bool,
    interfaces: usize,
}

/// Lists connected Corsair devices, then the graphics card and motherboard
/// lighting found at startup. Read-only: no device is opened.
#[tauri::command]
fn list_devices(state: State<AppState>) -> Result<Vec<DeviceInfo>, String> {
    let api = HidApi::new().map_err(|e| format!("Could not start the USB device library: {e}"))?;
    let mut out: Vec<DeviceInfo> = find_devices(&api)
        .into_iter()
        .map(|d| DeviceInfo {
            name: d.display_name().to_string(),
            product: d.product.clone(),
            usb_id: format!("{:04x}:{:04x}", CORSAIR_VID, d.pid),
            bus: format!("USB {:04x}:{:04x}", CORSAIR_VID, d.pid),
            kind: match d.model.map(|m| m.kind) {
                Some(DeviceKind::Keyboard) => "keyboard",
                Some(DeviceKind::Mouse) => "mouse",
                Some(DeviceKind::LightingController) => "lighting",
                None => "unknown",
            },
            recognized: d.is_supported(),
            experimental: d.model.is_some_and(|m| !m.tested),
            lighting_only: d.model.is_some_and(|m| m.kind == DeviceKind::Mouse) && !nightsword::model(d.pid).is_some_and(|m| m.full),
            interfaces: d.interfaces.len(),
        })
        .collect();
    if let Some(g) = &state.gpu {
        out.push(DeviceInfo { name: g.name.into(), product: g.name.into(), usb_id: String::new(), bus: format!("I2C bus {}", g.bus), kind: "gpu", recognized: true, experimental: false, lighting_only: true, interfaces: 1 });
    }
    if state.board.is_some() && asus_aura::present(&api) {
        let name = "ASUS Aura motherboard";
        out.push(DeviceInfo { name: name.into(), product: name.into(), usb_id: String::new(), bus: format!("USB {:04x}:{:04x}", asus_aura::ASUS_VID, asus_aura::SUPPORTED_PIDS[0]), kind: "motherboard", recognized: true, experimental: false, lighting_only: true, interfaces: 1 });
    }
    Ok(out)
}

pub(crate) struct AppState {
    engine: Engine,
    fans: Surface,
    keyboard: Surface,
    keys: Vec<Key>,
    mouse: Surface,
    gpu: Option<msi_gpu::FoundGpu>,
    board: Option<asus_aura::Layout>,
    /// The mouse model connected at startup (its zones shape the mouse's
    /// lighting); the Nightsword if none.
    mouse_model: &'static nightsword::MouseModel,
    remap: Remap,
}

/// The motherboard's lights, for the screen.
#[derive(Serialize)]
struct BoardInfo {
    /// Built-in lights (on the board and its 4-pin RGB sockets).
    fixed: usize,
    argb_headers: usize,
    /// LEDs assumed on a header whose count hasn't been set.
    default_argb_leds: u16,
    max_argb_leds: usize,
}

#[derive(Serialize)]
struct LightingState {
    store: LightingStore,
    /// What's plugged into the lighting controller's channels.
    layout: Vec<ChannelLayout>,
    /// Fans and strips that can be plugged in.
    pieces: &'static [PieceKind],
    /// The connected lighting controller: (name, channels, tested).
    node: Option<(&'static str, u8, bool)>,
    /// The K65's keys, in LED order.
    keys: Vec<Key>,
    /// The Nightsword's lighting zones, in LED order.
    mouse_zones: Vec<&'static str>,
    /// The mouse's buttons and the code each sends out of the box (0 = learn it).
    mouse_buttons: &'static [(&'static str, u16)],
    /// Keys offered for bindings: (group, [(code, name)]).
    catalog: &'static [(&'static str, &'static [(u16, &'static str)])],
    /// Why bindings aren't working, if they aren't.
    bindings_error: Option<String>,
    board: Option<BoardInfo>,
}

#[tauri::command]
fn get_lighting(state: State<AppState>) -> LightingState {
    LightingState {
        store: state.engine.config(),
        layout: effects::node_layout(&state.engine.config()),
        pieces: effects::PIECES,
        node: HidApi::new().ok().and_then(|api| find_devices(&api).into_iter().find_map(|d| lighting_node::model(d.pid))).map(|m| (m.1, m.2, m.3)),
        keys: state.keys.clone(),
        mouse_zones: state.mouse_model.zones.iter().map(|z| z.0).collect(),
        catalog: bindings::CATALOG,
        mouse_buttons: &nightsword::BUTTONS,
        bindings_error: state.remap.error(),
        board: state.board.map(|b| BoardInfo {
            fixed: b.fixed,
            argb_headers: b.argb_headers,
            default_argb_leds: engine::DEFAULT_ARGB_LEDS,
            max_argb_leds: asus_aura::MAX_ARGB_LEDS,
        }),
    }
}

/// Applies new lighting settings (all profiles). Returns them with anything
/// out of range fixed.
#[tauri::command]
fn set_lighting(app: tauri::AppHandle, state: State<AppState>, store: LightingStore) -> LightingStore {
    let store = store.sanitized(&state.fans, &state.keyboard, &state.mouse);
    state.engine.set_config(store.clone());
    state.remap.apply(&store, &state.engine);
    tray::refresh(&app); // profile names or the bindings switch may have changed
    store
}

#[derive(Serialize)]
struct Pressed {
    code: u16,
    name: String,
}

/// A name for any key code: Prism's friendly one if it has it.
fn key_name(code: u16) -> String {
    bindings::friendly_name(code).map(str::to_string).unwrap_or_else(|| prism_input::raw_name(code))
}

/// Waits (up to 8 s) for the next key or button press on the keyboard or
/// mouse and says which it was.
#[tauri::command]
async fn capture_input(app: tauri::AppHandle, device: InputDevice) -> Option<Pressed> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let store = state.engine.config();
        state
            .remap
            .capture(&store, &state.engine, device, Duration::from_secs(8))
            .map(|code| Pressed { code, name: key_name(code) })
    })
    .await
    .ok()
    .flatten()
}

/// Names for key codes, for bindings the catalog doesn't cover.
#[tauri::command]
fn key_names(codes: Vec<u16>) -> Vec<(u16, String)> {
    codes.into_iter().take(500).map(|c| (c, key_name(c))).collect()
}

/// Installed apps, for "open an app" bindings.
#[tauri::command]
fn list_apps() -> Vec<apps::App> {
    apps::list()
}

/// Starts recording a macro from the Corsair keyboard and mouse. Bindings
/// pause until recording stops (or a minute passes).
#[tauri::command]
fn macro_record_start(state: State<AppState>) {
    state.remap.record_start();
}

/// Stops recording and returns the recorded steps.
#[tauri::command]
async fn macro_record_stop(app: tauri::AppHandle) -> Vec<bindings::MacroStep> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let store = state.engine.config();
        state.remap.record_stop(&store, &state.engine)
    })
    .await
    .unwrap_or_default()
}

fn config_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    app.path().app_config_dir().map_err(|e| e.to_string())
}

/// Copies everything stored on the keyboard and mouse into a new backup
/// folder. Read-only: nothing on the devices changes.
#[tauri::command]
async fn backup_onboard(app: tauri::AppHandle) -> Result<backup::Manifest, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = config_dir(&app)?;
        app.state::<AppState>().engine.backup(&dir)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn backup_dir(app: &tauri::AppHandle, name: &str) -> Result<std::path::PathBuf, String> {
    if !backup::valid_name(name) {
        return Err("That isn't a backup name".into());
    }
    let dir = backup::root(&config_dir(app)?).join(name);
    if !dir.join("manifest.json").exists() {
        return Err("That backup doesn't exist".into());
    }
    Ok(dir)
}

/// What "Save to mouse" would write for the active profile. Read-only.
#[tauri::command]
async fn mouse_save_plan(app: tauri::AppHandle) -> Result<mouse_save::SavePlan, String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<AppState>().engine.mouse_plan())
        .await
        .map_err(|e| e.to_string())?
}

/// Saves the active profile's button keys and DPI into one of the mouse's
/// onboard profiles, after a backup. Changes the mouse's memory.
#[tauri::command]
async fn mouse_save(app: tauri::AppHandle, folder: u8, lighting: bool) -> Result<mouse_save::SaveReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = config_dir(&app)?;
        app.state::<AppState>().engine.mouse_save(folder, lighting, &dir)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// What "Save to keyboard" would write for the active profile. Read-only.
#[tauri::command]
async fn kb_save_plan(app: tauri::AppHandle) -> Result<kb_save::SavePlan, String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<AppState>().engine.kb_plan())
        .await
        .map_err(|e| e.to_string())?
}

/// Saves the active profile's key bindings into one of the keyboard's
/// onboard profiles, after a backup. Changes the keyboard's memory.
#[tauri::command]
async fn kb_save(app: tauri::AppHandle, root: u16, lighting: bool) -> Result<kb_save::SaveReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = config_dir(&app)?;
        app.state::<AppState>().engine.kb_save(root, lighting, &dir)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// What restoring a backup would change on the devices. Read-only.
#[tauri::command]
async fn restore_plan(app: tauri::AppHandle, name: String) -> Result<backup::Plan, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = backup_dir(&app, &name)?;
        app.state::<AppState>().engine.restore_plan(dir)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Restores a backup onto the devices (only files that differ), after
/// making a fresh backup. CHANGES THE DEVICES: the screen asks first.
#[tauri::command]
async fn restore_backup(app: tauri::AppHandle, name: String) -> Result<backup::RestoreReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = backup_dir(&app, &name)?;
        let config = config_dir(&app)?;
        app.state::<AppState>().engine.restore(dir, &config)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Every onboard backup, newest first.
#[tauri::command]
fn list_backups(app: tauri::AppHandle) -> Vec<backup::Manifest> {
    config_dir(&app).map(|d| backup::list(&d)).unwrap_or_default()
}

/// Opens the backups folder in the file manager.
/// Deletes one backup folder from this computer (the devices aren't
/// touched). The screen asks "are you sure?" first.
#[tauri::command]
fn delete_backup(app: tauri::AppHandle, name: String) -> Result<(), String> {
    let dir = backup_dir(&app, &name)?;
    // Only a real folder directly inside the backups folder, never a link.
    let meta = std::fs::symlink_metadata(&dir).map_err(|e| e.to_string())?;
    if !meta.is_dir() || dir.parent() != Some(backup::root(&config_dir(&app)?).as_path()) {
        return Err("That isn't a backup folder".into());
    }
    std::fs::remove_dir_all(&dir).map_err(|e| format!("Couldn't delete the backup: {e}"))
}

#[tauri::command]
fn open_backups(app: tauri::AppHandle) -> Result<(), String> {
    let dir = backup::root(&config_dir(&app)?);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::process::Command::new("xdg-open").arg(dir).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/* ---------- device reports ---------- */

fn parse_usb_id(id: &str) -> Result<(u16, u16), String> {
    let (v, p) = id.split_once(':').ok_or("That isn't a USB ID")?;
    let hex = |s: &str| u16::from_str_radix(s, 16).map_err(|_| "That isn't a USB ID".to_string());
    Ok((hex(v)?, hex(p)?))
}

/// Read-only facts about a device, for a report.
#[tauri::command]
async fn report_facts(usb_id: String) -> Result<report::Facts, String> {
    let (vid, pid) = parse_usb_id(&usb_id)?;
    tauri::async_runtime::spawn_blocking(move || report::facts(vid, pid)).await.map_err(|e| e.to_string())?
}

/// Starts recording presses from one device (the report's button walk).
#[tauri::command]
fn report_capture_start(state: State<AppState>, usb_id: String) -> Result<(), String> {
    let (_, pid) = parse_usb_id(&usb_id)?;
    state.engine.capture_start(pid);
    Ok(())
}

/// Stops recording; returns (code, name) for each key or button pressed.
#[tauri::command]
fn report_capture_stop(state: State<AppState>) -> Vec<(u16, String)> {
    state.engine.capture_stop().into_iter().map(|c| (c, key_name(c))).collect()
}

/// Light walk on a mouse: `zone` id lit in `color`, the others off; None
/// for every zone 1-6 in `color`.
#[tauri::command]
fn report_light_mouse(state: State<AppState>, zone: Option<u8>, color: String) -> Result<(), String> {
    let c = prism_core::Rgb::from_hex(&color).ok_or("Not a color")?;
    let off = prism_core::Rgb::new(0, 0, 0);
    let zones = (1..=6u8).map(|id| (id, if zone.map_or(true, |z| z == id) { c } else { off })).collect();
    state.engine.test_light(Some(engine::TestLight::Mouse(zones)));
    Ok(())
}

/// Light walk on a lighting controller: `channel` (0-based) lit in
/// `color` on its first `leds` LEDs, the other channel off.
#[tauri::command]
fn report_light_node(state: State<AppState>, channel: Option<u8>, color: String, leds: usize) -> Result<(), String> {
    let c = prism_core::Rgb::from_hex(&color).ok_or("Not a color")?;
    let colors = (0..lighting_node::CHANNELS).map(|ch| if channel.map_or(true, |x| x == ch) { c } else { prism_core::Rgb::new(0, 0, 0) }).collect();
    state.engine.test_light(Some(engine::TestLight::Node { colors, leds: leds.min(lighting_node::MAX_LEDS_PER_CHANNEL) }));
    Ok(())
}

/// Ends a light walk: back to the effects.
#[tauri::command]
fn report_light_off(state: State<AppState>) {
    state.engine.test_light(None);
}

/// Saves the reviewed report to Downloads; returns where.
#[tauri::command]
fn report_save(json: String, usb_id: String) -> Result<String, String> {
    report::save(&json, &usb_id)
}

/// Opens GitHub's new-issue page for Prism (nothing is attached or sent).
#[tauri::command]
fn report_open_issue(product: String) -> Result<(), String> {
    report::open_issue_page(&product)
}

/// Which device and tab to open on, from PRISM_START_VIEW ("keyboard:keys",
/// "lighting:light", ...). Used for taking screenshots; unset normally.
#[tauri::command]
fn start_view() -> Option<String> {
    std::env::var("PRISM_START_VIEW").ok().filter(|v| v.len() < 40)
}

/// Writes a screen-side error to Prism's own log (stderr), for debugging.
/// Nothing leaves the computer.
#[tauri::command]
fn ui_log(message: String) {
    let m: String = message.chars().filter(|c| !c.is_control()).take(500).collect();
    eprintln!("[screen] {m}");
}

/// Whether bindings are working, and why not if they aren't.
#[tauri::command]
fn bindings_status(state: State<AppState>) -> Option<String> {
    state.remap.error()
}

/// After restarting in the background, the old window's web helper
/// processes exit and are left as "zombie" entries until collected. Collect
/// them for the first half minute.
fn reap_leftovers() {
    extern "C" {
        fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
    }
    const WNOHANG: i32 = 1;
    std::thread::spawn(|| {
        for _ in 0..30 {
            let mut status = 0;
            // Safety: plain libc call with a valid pointer.
            while unsafe { waitpid(-1, &mut status, WNOHANG) } > 0 {}
            std::thread::sleep(Duration::from_secs(1));
        }
    });
}

fn main() {
    // WebKitGTK's DMA-BUF renderer crashes on NVIDIA under Wayland
    // ("Error 71 (Protocol error)"). Turn it off unless the user chose otherwise.
    // Set before any threads start.
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    // Started at login, or restarted after the window closed: stay in the
    // tray without opening the window.
    let hidden = std::env::args().any(|a| a == "--hidden");
    let resume_at: f64 = std::env::args()
        .find_map(|a| a.strip_prefix("--resume-at=").and_then(|v| v.parse().ok()))
        .unwrap_or(0.0);
    if hidden {
        reap_leftovers();
    }

    let app = tauri::Builder::default()
        // Only one Prism at a time (two would fight over the devices);
        // opening it again just shows the window.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| tray::show_window(app)))
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                tray::window_closed(window.app_handle());
            }
        })
        .setup(move |app| {
            // ~/.config/io.github.honeybadger.prism/lighting.json
            let save_to = app.path().app_config_dir().ok().map(|d| d.join("lighting.json"));
            let keys = k65_mini::keys();
            // The motherboard is opened now to learn what lights it has; the
            // engine then keeps it.
            let gpu = msi_gpu::find();
            let mouse_model = HidApi::new().ok().and_then(|api| nightsword::connected(&api)).unwrap_or(&nightsword::MODELS[0]);
            let aura = HidApi::new().ok().and_then(|api| asus_aura::AsusAura::open(&api).map_err(|e| eprintln!("motherboard: {e}")).ok());
            let board = aura.as_ref().map(|a| a.layout);
            let devices = Devices {
                fans: effects::node_max_surface(),
                keyboard: k65_mini::surface(&keys),
                keys: keys.clone(),
                mouse: nightsword::surface_for(mouse_model),
                gpu: gpu.clone(),
                aura,
            };
            let (fans, keyboard, mouse) = (devices.fans.clone(), devices.keyboard.clone(), devices.mouse.clone());
            let config = engine::load(&save_to, &devices);
            let remap = Remap::default();
            let engine = Engine::spawn(app.handle().clone(), config.clone(), devices, save_to, resume_at);
            remap.apply(&config, &engine);
            app.manage(AppState { engine, fans, keyboard, keys, mouse, gpu, board, mouse_model, remap });
            tray::create(app.handle())?;
            if !hidden {
                tray::show_window(app.handle());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_devices,
            get_lighting,
            set_lighting,
            capture_input,
            key_names,
            list_apps,
            bindings_status,
            macro_record_start,
            macro_record_stop,
            backup_onboard,
            list_backups,
            restore_plan,
            restore_backup,
            mouse_save_plan,
            mouse_save,
            kb_save_plan,
            kb_save,
            open_backups,
            delete_backup,
            start_view,
            report_facts,
            report_capture_start,
            report_capture_stop,
            report_light_mouse,
            report_light_node,
            report_light_off,
            report_save,
            report_open_issue,
            ui_log
        ])
        .build(tauri::generate_context!())
        .expect("Prism failed to start");

    app.run(|handle, event| match event {
        // Closing the window isn't quitting: keep running in the tray. (Quit
        // from the tray menu asks to exit with a code, which is let through.)
        RunEvent::ExitRequested { code: None, api, .. } => api.prevent_exit(),
        RunEvent::Exit => {
            let state = handle.state::<AppState>();
            state.remap.stop();
            state.engine.stop();
        }
        _ => {}
    });
}
