//! The lighting engine: a background thread that renders effects, sends them
//! to the Lighting Node Pro, and tells the screen what it sent.
//!
//! It keeps running when no device is connected (the preview still works) and
//! checks for the device every couple of seconds, so unplugging and
//! replugging just works.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use hidapi::HidApi;
use prism_core::effects::{render, ChannelLayout, Layer, LightingStore, Profile};
use prism_core::lighting_node::{LightingNode, CHANNELS, KEEPALIVE_INTERVAL};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

const FRAME_TIME: Duration = Duration::from_millis(33); // ~30 frames a second
const RECONNECT_EVERY: Duration = Duration::from_secs(2);
const SAVE_AFTER: Duration = Duration::from_secs(1);

enum Msg {
    Config(LightingStore),
    Stop,
}

#[derive(Clone, Serialize)]
struct Status {
    connected: bool,
}

pub struct Engine {
    tx: Sender<Msg>,
    thread: Mutex<Option<JoinHandle<()>>>,
    config: Mutex<LightingStore>,
    connected: Arc<AtomicBool>,
}

impl Engine {
    pub fn spawn(app: AppHandle, config: LightingStore, layout: Vec<ChannelLayout>, save_to: Option<PathBuf>) -> Self {
        let (tx, rx) = mpsc::channel();
        let connected = Arc::new(AtomicBool::new(false));
        let flag = connected.clone();
        let start_config = config.clone();

        let thread = std::thread::spawn(move || {
            let mut cfg = start_config;
            let mut api = HidApi::new().ok();
            let mut node: Option<LightingNode> = None;
            let mut last_try: Option<Instant> = None;
            let mut unsaved_since: Option<Instant> = None;
            let start = Instant::now();

            let set_connected = |on: bool| {
                if flag.swap(on, Ordering::SeqCst) != on {
                    let _ = app.emit("lighting-status", Status { connected: on });
                }
            };

            loop {
                // Look for the device if we don't have it.
                if node.is_none() && last_try.map_or(true, |t| t.elapsed() >= RECONNECT_EVERY) {
                    last_try = Some(Instant::now());
                    if let Some(api) = api.as_mut() {
                        let _ = api.refresh_devices();
                        if let Ok(n) = LightingNode::open(api) {
                            // We do brightness ourselves, so the device stays at full.
                            if (0..CHANNELS).all(|ch| n.set_brightness(ch, 100).is_ok()) {
                                node = Some(n);
                                set_connected(true);
                            }
                        }
                    }
                }

                let frame = render(&cfg, &layout, start.elapsed().as_secs_f64());

                if let Some(n) = &node {
                    let sent = frame
                        .iter()
                        .enumerate()
                        .filter(|(_, colors)| !colors.is_empty())
                        .all(|(ch, colors)| n.set_colors(ch as u8, colors).is_ok());
                    if !sent {
                        node = None; // unplugged, or something else took it over
                        set_connected(false);
                    }
                }
                let _ = app.emit("lighting-frame", &frame);

                if unsaved_since.is_some_and(|t| t.elapsed() >= SAVE_AFTER) {
                    save(&save_to, &cfg);
                    unsaved_since = None;
                }

                let mut wait = if cfg.is_animated() { FRAME_TIME } else { KEEPALIVE_INTERVAL };
                if unsaved_since.is_some() {
                    wait = wait.min(SAVE_AFTER);
                }
                if node.is_none() {
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
                        if let Some(n) = node {
                            let _ = n.release(); // back to the device's own effect
                        }
                        return;
                    }
                }
            }
        });

        Self { tx, thread: Mutex::new(Some(thread)), config: Mutex::new(config), connected }
    }

    pub fn config(&self) -> LightingStore {
        self.config.lock().unwrap().clone()
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    pub fn set_config(&self, config: LightingStore) {
        *self.config.lock().unwrap() = config.clone();
        let _ = self.tx.send(Msg::Config(config));
    }

    /// Stops the thread and hands the device back. Waits until it's done.
    pub fn stop(&self) {
        let _ = self.tx.send(Msg::Stop);
        if let Some(t) = self.thread.lock().unwrap().take() {
            let _ = t.join();
        }
    }
}

/// Reads saved lighting. Settings from before presets existed become a
/// "My settings" preset; anything unreadable falls back to the starter presets.
pub fn load(path: &Option<PathBuf>, layout: &[ChannelLayout]) -> LightingStore {
    let text = path.as_ref().and_then(|p| std::fs::read_to_string(p).ok());
    let Some(text) = text else { return LightingStore::starter(layout) };
    if let Ok(store) = serde_json::from_str::<LightingStore>(&text) {
        return store.sanitized(layout);
    }
    let mut store = LightingStore::starter(layout);
    if let Ok(old) = serde_json::from_str::<Layer>(&text) {
        let layer = Layer { name: "Main".into(), ..old };
        store.profiles.insert(0, Profile { name: "My settings".into(), layers: vec![layer] });
    }
    store.sanitized(layout)
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
