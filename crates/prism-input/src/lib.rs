//! Prism's input layer.
//!
//! Linux shows each keyboard and mouse as one or more "event devices"
//! (/dev/input/event*). To remap keys and buttons, Prism:
//!   1. takes over ("grabs") the Corsair event devices that have a key or
//!      button the user has bound, so the computer stops seeing them directly,
//!   2. reads everything they send,
//!   3. passes it on unchanged through a virtual device, except bound keys
//!      and buttons, which are replaced by their action.
//!
//! If Prism stops or crashes, Linux releases the grab by itself, so the
//! devices always come back.
//!
//! Privacy: only Corsair devices are opened, and nothing read here is stored
//! or sent anywhere. Events go straight back out through the virtual device.

use std::collections::{BTreeSet, HashMap};
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use evdev::uinput::{VirtualDevice, VirtualDeviceBuilder};
use evdev::{AttributeSet, Device, EventType, InputEvent, Key, RelativeAxisType};

pub const CORSAIR_VENDOR: u16 = 0x1b1c;
/// The name the virtual device shows up as.
pub const VIRTUAL_NAME: &str = "Prism virtual input";

/// One Corsair event device.
pub struct InputDevice {
    pub path: PathBuf,
    pub name: String,
    pub product: u16,
    pub device: Device,
}

impl InputDevice {
    /// Whether it sends key or button presses (as opposed to only movement).
    pub fn has_keys(&self) -> bool {
        self.device.supported_keys().is_some_and(|k| k.iter().next().is_some())
    }

    fn sends(&self, code: u16) -> bool {
        self.device.supported_keys().is_some_and(|k| k.contains(Key::new(code)))
    }
}

/// Every Corsair event device this user can open (never Prism's own
/// virtual device).
pub fn corsair_devices() -> Vec<InputDevice> {
    let mut out: Vec<InputDevice> = evdev::enumerate()
        .filter(|(_, d)| d.input_id().vendor() == CORSAIR_VENDOR && d.name() != Some(VIRTUAL_NAME))
        .map(|(path, device)| InputDevice {
            name: device.name().unwrap_or("").to_string(),
            product: device.input_id().product(),
            path,
            device,
        })
        .collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// What a bound key or button does instead. Codes are Linux key codes
/// (the same numbers `prism-input watch` shows names for).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Acts as another key or button.
    Key(u16),
    /// Holds several keys together, e.g. Ctrl+Shift+S. Pressed in order and
    /// released in reverse.
    Combo(Vec<u16>),
    /// Opens an installed app by its desktop id, using `gtk-launch`.
    Launch(String),
    /// Does nothing.
    Disabled,
}

impl Action {
    fn keys(&self) -> Vec<u16> {
        match self {
            Action::Key(k) => vec![*k],
            Action::Combo(ks) => ks.clone(),
            Action::Launch(_) | Action::Disabled => vec![],
        }
    }
}

/// Opens an app by desktop id without going through a shell. The id must be
/// plain (letters, digits, `.`, `_`, `-`), so it can't be used to run
/// anything else.
fn launch(app: &str) {
    if app.is_empty() || !app.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)) {
        return;
    }
    let child = std::process::Command::new("gtk-launch")
        .arg(app)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    if let Ok(mut c) = child {
        // Collect the launcher when it finishes so it doesn't linger.
        std::thread::spawn(move || {
            let _ = c.wait();
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    /// Which Corsair device it's on (USB product id), so the same key on two
    /// devices can do different things.
    pub product: u16,
    /// The key or button that's bound.
    pub input: u16,
    pub action: Action,
}

/// Key state values in key events.
const RELEASED: i32 = 0;
const PRESSED: i32 = 1;

/// What to send for one key event from a grabbed device: the event itself,
/// or its action. Key repeats of a combo or disabled key are dropped.
fn translate(code: u16, value: i32, bindings: &HashMap<u16, Action>) -> Vec<(u16, i32)> {
    match bindings.get(&code) {
        None => vec![(code, value)],
        Some(Action::Key(k)) => vec![(*k, value)],
        Some(Action::Disabled) | Some(Action::Launch(_)) => vec![],
        Some(Action::Combo(keys)) => match value {
            PRESSED => keys.iter().map(|&k| (k, PRESSED)).collect(),
            RELEASED => keys.iter().rev().map(|&k| (k, RELEASED)).collect(),
            _ => vec![],
        },
    }
}

/// Remaps keys while it's alive. Dropping it (or calling [`Remapper::stop`])
/// hands every device back.
pub struct Remapper {
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    /// Paths of the event devices taken over, for display.
    pub grabbed: Vec<PathBuf>,
}

impl Remapper {
    /// Starts remapping. Only devices that send a bound key are taken over;
    /// with no bindings, nothing is touched.
    pub fn start(bindings: &[Binding]) -> std::io::Result<Self> {
        let mut by_product: HashMap<u16, HashMap<u16, Action>> = HashMap::new();
        for b in bindings {
            by_product.entry(b.product).or_default().insert(b.input, b.action.clone());
        }

        // A device is taken over only if it sends a bound key.
        let mut chosen: Vec<(InputDevice, HashMap<u16, Action>)> = Vec::new();
        for d in corsair_devices() {
            if let Some(map) = by_product.get(&d.product) {
                if map.keys().any(|&k| d.sends(k)) {
                    chosen.push((d, map.clone()));
                }
            }
        }
        if chosen.is_empty() {
            return Ok(Self { stop: Arc::new(AtomicBool::new(false)), threads: vec![], grabbed: vec![] });
        }

        // The virtual device can send everything the taken-over devices can,
        // plus every key an action uses.
        let mut keys = AttributeSet::<Key>::new();
        let mut rel = AttributeSet::<RelativeAxisType>::new();
        for (d, map) in &chosen {
            if let Some(ks) = d.device.supported_keys() {
                ks.iter().for_each(|k| keys.insert(k));
            }
            if let Some(rs) = d.device.supported_relative_axes() {
                rs.iter().for_each(|r| rel.insert(r));
            }
            map.values().flat_map(Action::keys).for_each(|k| keys.insert(Key::new(k)));
        }
        let mut builder = VirtualDeviceBuilder::new()?.name(VIRTUAL_NAME).with_keys(&keys)?;
        if rel.iter().next().is_some() {
            builder = builder.with_relative_axes(&rel)?;
        }
        let out = Arc::new(Mutex::new(builder.build()?));

        let stop = Arc::new(AtomicBool::new(false));
        let mut threads = Vec::new();
        let mut grabbed = Vec::new();
        for (mut d, map) in chosen {
            d.device.grab()?;
            grabbed.push(d.path.clone());
            let (stop, out) = (stop.clone(), out.clone());
            threads.push(std::thread::spawn(move || pump(d.device, map, out, stop)));
        }
        Ok(Self { stop, threads, grabbed })
    }

    /// Hands every device back and waits for that to finish.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

impl Drop for Remapper {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Reads one taken-over device and re-sends its events until told to stop.
fn pump(mut dev: Device, bindings: HashMap<u16, Action>, out: Arc<Mutex<VirtualDevice>>, stop: Arc<AtomicBool>) {
    let fd = dev.as_raw_fd();
    while !stop.load(Ordering::SeqCst) {
        // Wait up to 100 ms for input so we notice `stop` promptly.
        if !readable(fd, 100) {
            continue;
        }
        let Ok(events) = dev.fetch_events() else { break }; // unplugged
        let mut batch: Vec<InputEvent> = Vec::new();
        for ev in events {
            match ev.event_type() {
                EventType::KEY => {
                    if let (Some(Action::Launch(app)), PRESSED) = (bindings.get(&ev.code()), ev.value()) {
                        launch(app);
                    }
                    for (code, value) in translate(ev.code(), ev.value(), &bindings) {
                        batch.push(InputEvent::new(EventType::KEY, code, value));
                    }
                }
                EventType::RELATIVE => batch.push(ev),
                _ => {} // sync and scan-code events; `emit` adds its own sync
            }
        }
        if !batch.is_empty() {
            if let Ok(mut o) = out.lock() {
                let _ = o.emit(&batch);
            }
        }
    }
    let _ = dev.ungrab();
}

#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

extern "C" {
    fn poll(fds: *mut PollFd, nfds: u64, timeout: i32) -> i32;
}

fn readable(fd: i32, timeout_ms: i32) -> bool {
    const POLLIN: i16 = 1;
    let mut p = PollFd { fd, events: POLLIN, revents: 0 };
    // Safety: one valid pollfd, and the count says one.
    unsafe { poll(&mut p, 1, timeout_ms) > 0 && p.revents & POLLIN != 0 }
}

/// Waits up to `timeout` for the next key or button press on a Corsair
/// device with USB product id `product`, and returns its code. Nothing is
/// taken over while waiting.
pub fn capture_press(product: u16, timeout: std::time::Duration) -> Option<u16> {
    let mut devs: Vec<Device> =
        corsair_devices().into_iter().filter(|d| d.product == product && d.has_keys()).map(|d| d.device).collect();
    let end = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < end {
        for d in &mut devs {
            if !readable(d.as_raw_fd(), 20) {
                continue;
            }
            let Ok(events) = d.fetch_events() else { continue };
            for ev in events {
                if ev.event_type() == EventType::KEY && ev.value() == PRESSED {
                    return Some(ev.code());
                }
            }
        }
    }
    None
}

/// A readable name for any key code, e.g. "KEY_KPSLASH" becomes "Kpslash".
pub fn raw_name(code: u16) -> String {
    let n = format!("{:?}", Key::new(code));
    let n = n.trim_start_matches("KEY_").trim_start_matches("BTN_");
    let mut out = n.to_ascii_lowercase();
    if let Some(f) = out.get_mut(0..1) {
        f.make_ascii_uppercase();
    }
    out
}

/// Looks up a key or button by its Linux name, e.g. "KEY_A" or "BTN_SIDE".
pub fn key_code(name: &str) -> Option<u16> {
    let wanted = name.to_ascii_uppercase();
    (0..0x300u16).find(|&c| format!("{:?}", Key::new(c)) == wanted)
}

/// Every key code used by `bindings`, for display.
pub fn codes_in(bindings: &[Binding]) -> BTreeSet<u16> {
    bindings.iter().flat_map(|b| std::iter::once(b.input).chain(b.action.keys())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: u16 = 30;
    const B: u16 = 48;
    const CTRL: u16 = 29;
    const SHIFT: u16 = 42;
    const S: u16 = 31;

    fn map(pairs: &[(u16, Action)]) -> HashMap<u16, Action> {
        pairs.iter().cloned().collect()
    }

    #[test]
    fn unbound_keys_pass_through() {
        let m = map(&[(A, Action::Key(B))]);
        assert_eq!(translate(S, PRESSED, &m), vec![(S, PRESSED)]);
        assert_eq!(translate(S, 2, &m), vec![(S, 2)]); // repeats too
    }

    #[test]
    fn key_to_key_keeps_press_release_and_repeat() {
        let m = map(&[(A, Action::Key(B))]);
        assert_eq!(translate(A, PRESSED, &m), vec![(B, PRESSED)]);
        assert_eq!(translate(A, 2, &m), vec![(B, 2)]);
        assert_eq!(translate(A, RELEASED, &m), vec![(B, RELEASED)]);
    }

    #[test]
    fn combo_presses_in_order_and_releases_in_reverse() {
        let m = map(&[(A, Action::Combo(vec![CTRL, SHIFT, S]))]);
        assert_eq!(translate(A, PRESSED, &m), vec![(CTRL, 1), (SHIFT, 1), (S, 1)]);
        assert_eq!(translate(A, 2, &m), vec![]);
        assert_eq!(translate(A, RELEASED, &m), vec![(S, 0), (SHIFT, 0), (CTRL, 0)]);
    }

    #[test]
    fn disabled_sends_nothing() {
        let m = map(&[(A, Action::Disabled)]);
        assert!(translate(A, PRESSED, &m).is_empty());
        assert!(translate(A, RELEASED, &m).is_empty());
    }

    #[test]
    fn launch_sends_no_key_events() {
        let m = map(&[(A, Action::Launch("firefox".into()))]);
        assert!(translate(A, PRESSED, &m).is_empty());
        assert!(translate(A, RELEASED, &m).is_empty());
    }

    #[test]
    fn raw_names() {
        assert_eq!(raw_name(98), "Kpslash");
        assert_eq!(raw_name(0x113), "Side");
    }

    #[test]
    fn names_to_codes() {
        assert_eq!(key_code("KEY_A"), Some(A));
        assert_eq!(key_code("key_capslock"), Some(58));
        assert_eq!(key_code("BTN_SIDE"), Some(0x113));
        assert_eq!(key_code("KEY_NOT_REAL"), None);
    }
}
