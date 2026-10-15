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
use prism_core::effects::{render_layers, Layer, LightingStore, Profile, Surface};
use prism_core::k65_mini::{self, K65Mini, Key};
use prism_core::lighting_node::{LightingNode, CHANNELS, KEEPALIVE_INTERVAL};
use prism_core::nightsword::{self, Nightsword};
use prism_core::Rgb;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

const FRAME_TIME: Duration = Duration::from_millis(33); // ~30 frames a second
const RECONNECT_EVERY: Duration = Duration::from_secs(2);
const SAVE_AFTER: Duration = Duration::from_secs(1);

enum Msg {
    Config(LightingStore),
    Stop,
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
    pub fn spawn(app: AppHandle, config: LightingStore, devices: Devices, save_to: Option<PathBuf>) -> Self {
        let (tx, rx) = mpsc::channel();
        let start_config = config.clone();

        let thread = std::thread::spawn(move || {
            let mut cfg = start_config;
            let mut api = HidApi::new().ok();
            let mut node: Link<LightingNode> = Link::new();
            let mut kb: Link<K65Mini> = Link::new();
            let mut mouse: Link<Nightsword> = Link::new();
            let mut unsaved_since: Option<Instant> = None;
            let start = Instant::now();
            let empty = Profile { name: String::new(), layers: vec![], keyboard: vec![], mouse: vec![], bindings: vec![] };

            loop {
                node.reconnect(&mut api, |api| {
                    let n = LightingNode::open(api).ok()?;
                    // We do brightness ourselves, so the device stays at full.
                    (0..CHANNELS).all(|ch| n.set_brightness(ch, 100).is_ok()).then_some(n)
                });
                kb.reconnect(&mut api, |api| K65Mini::open(api).ok());
                mouse.reconnect(&mut api, |api| Nightsword::open(api).ok());

                let t = start.elapsed().as_secs_f64();
                let profile = cfg.active_profile().unwrap_or(&empty);
                let fans = render_layers(&profile.layers, &devices.fans, cfg.brightness, t);
                let keys = render_layers(&profile.keyboard, &devices.keyboard, cfg.brightness, t);
                let keys: &[Rgb] = keys.first().map_or(&[], Vec::as_slice);
                let zones = render_layers(&profile.mouse, &devices.mouse, cfg.brightness, t);
                let zones: &[Rgb] = zones.first().map_or(&[], Vec::as_slice);

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
                if let Some(k) = &kb.dev {
                    if k.set_colors(&k65_mini::slot_colors(&devices.keys, keys)).is_err() {
                        kb.dev = None;
                    }
                }
                if let (Some(m), Ok(z)) = (&mouse.dev, <[Rgb; nightsword::ZONE_COUNT]>::try_from(zones)) {
                    if m.set_colors(&z).is_err() {
                        mouse.dev = None;
                    }
                }
                let _ = app.emit("lighting-frame", Frame { node: &fans, keyboard: keys, mouse: zones });

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

        Self { tx, thread: Mutex::new(Some(thread)), config: Mutex::new(config) }
    }

    pub fn config(&self) -> LightingStore {
        self.config.lock().unwrap().clone()
    }

    pub fn set_config(&self, config: LightingStore) {
        *self.config.lock().unwrap() = config.clone();
        let _ = self.tx.send(Msg::Config(config));
    }

    /// Stops the thread and hands the devices back. Waits until it's done.
    pub fn stop(&self) {
        let _ = self.tx.send(Msg::Stop);
        if let Some(t) = self.thread.lock().unwrap().take() {
            let _ = t.join();
        }
    }
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
        store.profiles.insert(0, Profile { name: "My settings".into(), layers: vec![layer], keyboard, mouse, bindings: vec![] });
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
