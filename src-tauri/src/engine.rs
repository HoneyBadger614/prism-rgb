//! The lighting engine: a background thread that renders effects, sends them
//! to the Lighting Node Pro and the K65 keyboard, and tells the screen what
//! it sent.
//!
//! It keeps running when a device isn't connected (the preview still works)
//! and looks for missing devices every couple of seconds, so unplugging and
//! replugging just works.

use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use hidapi::HidApi;
use prism_core::bindings::{DpiOp, DpiSettings};
use prism_core::effects::{render_layers, Layer, LightingStore, Profile, Surface};
use prism_core::k65_mini::{self, K65Mini, Key};
use prism_core::lighting_node::{LightingNode, CHANNELS, KEEPALIVE_INTERVAL};
use prism_core::nightsword::{self, Nightsword};
use prism_core::Rgb;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

const FRAME_TIME: Duration = Duration::from_millis(33); // ~30 frames a second
const RECONNECT_EVERY: Duration = Duration::from_secs(2);
const SAVE_AFTER: Duration = Duration::from_secs(1);

enum Msg {
    Config(LightingStore),
    /// Which mouse buttons should report to the computer (for bindings).
    MouseReporting(Vec<&'static str>),
    /// A DPI binding was pressed (true) or let go.
    Dpi(DpiOp, bool),
    /// Save settings now; reply when done.
    Flush(Sender<()>),
    /// Back up both devices' onboard memory into this folder; reply with what was saved.
    Backup(PathBuf, String, Sender<crate::backup::Manifest>),
    Stop,
}

/// DPI ops as numbers, for bindings that notify the app.
pub fn dpi_op_id(op: DpiOp) -> u32 {
    match op {
        DpiOp::Up => 1,
        DpiOp::Down => 2,
        DpiOp::Cycle => 3,
        DpiOp::Sniper => 4,
    }
}

fn dpi_op_from(id: u32) -> Option<DpiOp> {
    Some(match id {
        1 => DpiOp::Up,
        2 => DpiOp::Down,
        3 => DpiOp::Cycle,
        4 => DpiOp::Sniper,
        _ => return None,
    })
}

/// What the screen is told about the mouse's DPI.
#[derive(Clone, Serialize)]
struct DpiStatus {
    /// Stage 1-3, or 0 for sniper; None when Prism isn't controlling DPI.
    stage: Option<u8>,
    dpi: Option<u16>,
}

/// What the screen gets each frame: exactly what each device was sent.
#[derive(Clone, Serialize)]
struct Frame<'a> {
    node: &'a [Vec<Rgb>],
    keyboard: &'a [Rgb],
    mouse: &'a [Rgb],
}

/// The devices Prism drives, plus how they're laid out.
pub struct Devices {
    pub fans: Surface,
    pub keyboard: Surface,
    pub keys: Vec<Key>,
    pub mouse: Surface,
}

pub struct Engine {
    tx: Sender<Msg>,
    /// When effects started, so a restarted Prism can carry on from the same
    /// point instead of jumping back to the beginning.
    started: Instant,
    thread: Mutex<Option<JoinHandle<()>>>,
    config: Mutex<LightingStore>,
}

/// One device the engine may or may not have open right now.
struct Link<T> {
    dev: Option<T>,
    last_try: Option<Instant>,
}

impl<T> Link<T> {
    fn new() -> Self {
        Self { dev: None, last_try: None }
    }

    /// Tries `open` if the device is missing and it's been a while.
    fn reconnect(&mut self, api: &mut Option<HidApi>, open: impl Fn(&HidApi) -> Option<T>) {
        if self.dev.is_some() || self.last_try.is_some_and(|t| t.elapsed() < RECONNECT_EVERY) {
            return;
        }
        self.last_try = Some(Instant::now());
        if let Some(api) = api.as_mut() {
            let _ = api.refresh_devices();
            self.dev = open(api);
        }
    }
}

impl Engine {
    /// `resume_at` is how many seconds into the effects to start (after a
    /// background restart), so animations don't jump.
    pub fn spawn(app: AppHandle, config: LightingStore, devices: Devices, save_to: Option<PathBuf>, resume_at: f64) -> Self {
        let started = Instant::now().checked_sub(Duration::from_secs_f64(resume_at.clamp(0.0, 1e7))).unwrap_or_else(Instant::now);
        let (tx, rx) = mpsc::channel();
        let start_config = config.clone();

        let thread = std::thread::spawn(move || {
            let mut cfg = start_config;
            let mut api = HidApi::new().ok();
            let mut node: Link<LightingNode> = Link::new();
            let mut kb: Link<K65Mini> = Link::new();
            let mut mouse: Link<Nightsword> = Link::new();
            let mut unsaved_since: Option<Instant> = None;
            let mut reporting: Vec<&'static str> = Vec::new();
            // DPI: what's been sent to the mouse, the stage it's on, and
            // whether sniper is held.
            let mut dpi_applied: Option<DpiSettings> = None;
            let mut stage: u8 = 0;
            let mut sniper = false;
            // After a background restart the devices are still in Prism's
            // control, so the first connection skips "take control".
            let mut resuming = resume_at > 0.0;
            let start = started;
            let empty = Profile { name: String::new(), layers: vec![], keyboard: vec![], mouse: vec![], bindings: vec![], dpi: Default::default() };

            loop {
                // Work out this frame first, so a device that (re)connects
                // gets its colors straight away. Its "take control" message
                // blanks its lights until colors arrive.
                let t = start.elapsed().as_secs_f64();
                let profile = cfg.active_profile().unwrap_or(&empty);
                let fans = render_layers(&profile.layers, &devices.fans, cfg.brightness, t);
                let keys = render_layers(&profile.keyboard, &devices.keyboard, cfg.brightness, t);
                let keys: &[Rgb] = keys.first().map_or(&[], Vec::as_slice);
                let zones = render_layers(&profile.mouse, &devices.mouse, cfg.brightness, t);
                let zones: &[Rgb] = zones.first().map_or(&[], Vec::as_slice);

                node.reconnect(&mut api, |api| {
                    let n = LightingNode::open(api).ok()?;
                    if resuming {
                        n.assume_software();
                        return Some(n);
                    }
                    // We do brightness ourselves, so the device stays at full.
                    (0..CHANNELS).all(|ch| n.set_brightness(ch, 100).is_ok()).then_some(n)
                });
                if let Some(n) = &node.dev {
                    let sent = fans
                        .iter()
                        .enumerate()
                        .filter(|(_, colors)| !colors.is_empty())
                        .all(|(ch, colors)| n.set_colors(ch as u8, colors).is_ok());
                    if !sent {
                        node.dev = None; // unplugged, or something else took it over
                    }
                }

                kb.reconnect(&mut api, |api| K65Mini::open_with(api, !resuming).ok());
                if let Some(k) = &kb.dev {
                    if k.set_colors(&k65_mini::slot_colors(&devices.keys, keys)).is_err() {
                        kb.dev = None;
                    }
                }

                let had_mouse = mouse.dev.is_some();
                mouse.reconnect(&mut api, |api| Nightsword::open_with(api, !resuming).ok());
                resuming = false; // only the very first connection
                if let (Some(m), Ok(z)) = (&mouse.dev, <[Rgb; nightsword::ZONE_COUNT]>::try_from(zones)) {
                    if m.set_colors(&z).is_err() {
                        mouse.dev = None;
                    }
                }
                // The mouse's slower setup comes after its colors.
                if !had_mouse && mouse.dev.is_some() {
                    dpi_applied = None; // a (re)connected mouse needs its DPI set again
                    if !reporting.is_empty() {
                        let _ = mouse.dev.as_ref().map(|m| m.report_buttons(&reporting));
                    }
                }

                // Keep the mouse's DPI in step with the profile.
                let dpi_want = profile.dpi.clone();
                if let Some(m) = &mouse.dev {
                    if dpi_want.enabled && dpi_applied.as_ref() != Some(&dpi_want) {
                        let first = dpi_applied.as_ref().map_or(true, |d| !d.enabled);
                        if first || dpi_applied.as_ref().is_some_and(|d| d.start != dpi_want.start) {
                            stage = dpi_want.start;
                        }
                        let _ = m.set_dpi_stages(dpi_want.stages, dpi_want.sniper);
                        let _ = m.select_dpi(if sniper { 0 } else { stage });
                        dpi_applied = Some(dpi_want.clone());
                        emit_dpi(&app, &dpi_want, stage, sniper);
                    } else if !dpi_want.enabled && dpi_applied.is_some() {
                        // Give DPI back: hand the mouse over and pick it up again.
                        let _ = m.release();
                        mouse.dev = None;
                        mouse.last_try = None;
                        dpi_applied = None;
                        sniper = false;
                        let _ = app.emit("dpi-status", DpiStatus { stage: None, dpi: None });
                    }
                }

                // Only bother the screen when there is one.
                if !app.webview_windows().is_empty() {
                    let _ = app.emit("lighting-frame", Frame { node: &fans, keyboard: keys, mouse: zones });
                }

                if unsaved_since.is_some_and(|t| t.elapsed() >= SAVE_AFTER) {
                    save(&save_to, &cfg);
                    unsaved_since = None;
                }

                let mut wait = if cfg.is_animated() { FRAME_TIME } else { KEEPALIVE_INTERVAL };
                if unsaved_since.is_some() {
                    wait = wait.min(SAVE_AFTER);
                }
                if node.dev.is_none() || kb.dev.is_none() || mouse.dev.is_none() {
                    wait = wait.min(RECONNECT_EVERY);
                }

                match rx.recv_timeout(wait) {
                    Ok(Msg::Config(c)) => {
                        cfg = c;
                        unsaved_since = Some(Instant::now());
                    }
                    Ok(Msg::Backup(dir, name, done)) => {
                        use crate::backup::{save_files, write_manifest, DeviceBackup, Manifest};
                        let mut keyboard = DeviceBackup::default();
                        if let Some(k) = &kb.dev {
                            keyboard.read = true;
                            let mut ok = Vec::new();
                            for (id, r) in k.read_all_files() {
                                match r {
                                    Ok(data) => ok.push((format!("{id:04x}.bin"), data)),
                                    Err(e) => keyboard.error = Some(format!("file {id:#06x}: {e}")),
                                }
                            }
                            match save_files(&dir, "k65", &ok) {
                                Ok(files) => keyboard.files = files,
                                Err(e) => keyboard.error = Some(e.to_string()),
                            }
                        }
                        let mut mouse_b = DeviceBackup::default();
                        if let (Some(m), Some(api)) = (&mouse.dev, api.as_ref()) {
                            mouse_b.read = true;
                            match m.read_all_files(api) {
                                Ok(files) => {
                                    let named: Vec<(String, Vec<u8>)> =
                                        files.into_iter().map(|(f, n, d)| (format!("{f}_{n}"), d)).collect();
                                    match save_files(&dir, "nightsword", &named) {
                                        Ok(saved) => mouse_b.files = saved,
                                        Err(e) => mouse_b.error = Some(e.to_string()),
                                    }
                                }
                                Err(e) => mouse_b.error = Some(e.to_string()),
                            }
                        }
                        // "2026-10-15_23-40-00" -> "2026-10-15 23:40:00"
                        let created = match name.split_once('_') {
                            Some((date, time)) => format!("{date} {}", time.replace('-', ":")),
                            None => name.clone(),
                        };
                        let m = Manifest { name, created, keyboard, mouse: mouse_b };
                        let _ = write_manifest(&dir, &m);
                        let _ = done.send(m);
                    }
                    Ok(Msg::Flush(done)) => {
                        save(&save_to, &cfg);
                        unsaved_since = None;
                        let _ = done.send(());
                    }
                    Ok(Msg::Dpi(op, down)) => {
                        if let (Some(m), Some(d)) = (&mouse.dev, dpi_applied.as_ref().filter(|d| d.enabled)) {
                            let before = (stage, sniper);
                            match (op, down) {
                                (DpiOp::Up, true) => stage = (stage + 1).min(3),
                                (DpiOp::Down, true) => stage = stage.saturating_sub(1).max(1),
                                (DpiOp::Cycle, true) => stage = stage % 3 + 1,
                                (DpiOp::Sniper, held) => sniper = held,
                                _ => {}
                            }
                            if (stage, sniper) != before {
                                let _ = m.select_dpi(if sniper { 0 } else { stage });
                                emit_dpi(&app, d, stage, sniper);
                            }
                        }
                    }
                    Ok(Msg::MouseReporting(names)) => {
                        if names != reporting {
                            reporting = names;
                            if let Some(m) = &mouse.dev {
                                let _ = m.report_buttons(&reporting);
                            }
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => {
                        if unsaved_since.is_some() {
                            save(&save_to, &cfg);
                        }
                        // Back to each device's own lighting.
                        if let Some(n) = node.dev {
                            let _ = n.release();
                        }
                        if let Some(k) = kb.dev {
                            let _ = k.release();
                        }
                        if let Some(m) = mouse.dev {
                            let _ = m.release();
                        }
                        return;
                    }
                }
            }
        });

        Self { tx, started, thread: Mutex::new(Some(thread)), config: Mutex::new(config) }
    }

    pub fn config(&self) -> LightingStore {
        self.config.lock().unwrap().clone()
    }

    pub fn set_config(&self, config: LightingStore) {
        *self.config.lock().unwrap() = config.clone();
        let _ = self.tx.send(Msg::Config(config));
    }

    /// Sets which mouse buttons report to the computer instead of the mouse
    /// handling them. Everything else stays with the mouse.
    pub fn set_mouse_reporting(&self, names: Vec<&'static str>) {
        let _ = self.tx.send(Msg::MouseReporting(names));
    }

    /// Backs up both devices' onboard memory (read-only) into a new folder.
    pub fn backup(&self, config_dir: &std::path::Path) -> Result<crate::backup::Manifest, String> {
        let (name, dir) = crate::backup::new_folder(config_dir).map_err(|e| e.to_string())?;
        let (tx, rx) = mpsc::channel();
        self.tx.send(Msg::Backup(dir, name, tx)).map_err(|_| "Prism's device engine isn't running".to_string())?;
        rx.recv_timeout(Duration::from_secs(90)).map_err(|_| "The backup took too long".to_string())
    }

    /// Seconds since effects started.
    pub fn elapsed(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// Saves settings to disk now, waiting (briefly) until it's done.
    pub fn flush(&self) {
        let (tx, rx) = mpsc::channel();
        if self.tx.send(Msg::Flush(tx)).is_ok() {
            let _ = rx.recv_timeout(Duration::from_secs(2));
        }
    }

    /// A callback for bindings: DPI action numbers become DPI changes.
    pub fn dpi_notifier(&self) -> prism_input::Notifier {
        let tx = Mutex::new(self.tx.clone());
        std::sync::Arc::new(move |id, down| {
            if let (Some(op), Ok(tx)) = (dpi_op_from(id), tx.lock()) {
                let _ = tx.send(Msg::Dpi(op, down));
            }
        })
    }

    /// Stops the thread and hands the devices back. Waits until it's done.
    pub fn stop(&self) {
        let _ = self.tx.send(Msg::Stop);
        if let Some(t) = self.thread.lock().unwrap().take() {
            let _ = t.join();
        }
    }
}

fn emit_dpi(app: &AppHandle, d: &DpiSettings, stage: u8, sniper: bool) {
    let (stage, dpi) = if sniper { (0, d.sniper) } else { (stage, d.stages[(stage.clamp(1, 3) - 1) as usize]) };
    let _ = app.emit("dpi-status", DpiStatus { stage: Some(stage), dpi: Some(dpi) });
}

/// Reads saved lighting. Settings from before profiles existed become a
/// "My settings" profile; anything unreadable falls back to the starter.
pub fn load(path: &Option<PathBuf>, devices: &Devices) -> LightingStore {
    let text = path.as_ref().and_then(|p| std::fs::read_to_string(p).ok());
    let Some(text) = text else { return LightingStore::starter() };
    if let Ok(store) = serde_json::from_str::<LightingStore>(&text) {
        return store.sanitized(&devices.fans, &devices.keyboard, &devices.mouse);
    }
    let mut store = LightingStore::starter();
    if let Ok(old) = serde_json::from_str::<Layer>(&text) {
        let layer = Layer { name: "Main".into(), ..old };
        let (keyboard, mouse) = (store.profiles[0].keyboard.clone(), store.profiles[0].mouse.clone());
        store.profiles.insert(0, Profile { name: "My settings".into(), layers: vec![layer], keyboard, mouse, bindings: vec![], dpi: Default::default() });
    }
    store.sanitized(&devices.fans, &devices.keyboard, &devices.mouse)
}

fn save(path: &Option<PathBuf>, cfg: &LightingStore) {
    let Some(path) = path else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string_pretty(cfg) {
        // Write to a temp file first so a crash can't leave a half-written file.
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}
