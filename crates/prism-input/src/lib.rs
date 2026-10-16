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

use std::collections::{BTreeSet, HashMap, HashSet};
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
    /// Plays a macro.
    Macro { steps: Arc<Vec<Step>>, mode: Mode },
    /// Tells the app something happened (press and release), for actions
    /// it handles itself, like changing the mouse's DPI. The number says
    /// which action.
    Notify(u32),
    /// Does nothing.
    Disabled,
}

/// One step of a macro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Down(u16),
    Up(u16),
    /// Milliseconds.
    Wait(u32),
}

/// How a macro plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Once,
    /// Over and over while held.
    Repeat,
    /// Press to start repeating, press again to stop.
    Toggle,
}

/// Called with (action number, pressed) for [`Action::Notify`].
pub type Notifier = Arc<dyn Fn(u32, bool) + Send + Sync>;

fn notify(n: &Option<Notifier>, action: Option<&Action>, value: i32) {
    if let (Some(cb), Some(Action::Notify(id)), PRESSED | RELEASED) = (n, action, value) {
        cb(*id, value == PRESSED);
    }
}

impl Action {
    fn keys(&self) -> Vec<u16> {
        match self {
            Action::Key(k) => vec![*k],
            Action::Combo(ks) => ks.clone(),
            Action::Macro { steps, .. } => steps
                .iter()
                .filter_map(|s| match s {
                    Step::Down(k) | Step::Up(k) => Some(*k),
                    Step::Wait(_) => None,
                })
                .collect(),
            Action::Launch(_) | Action::Notify(_) | Action::Disabled => vec![],
        }
    }
}

/// Plays macros through a shared virtual device, each on its own thread so
/// waits don't hold anything else up.
struct Macros {
    out: Arc<Mutex<VirtualDevice>>,
    /// Per bound key: (playing, asked to stop).
    players: HashMap<u16, (Arc<AtomicBool>, Arc<AtomicBool>)>,
}

impl Macros {
    fn new(out: Arc<Mutex<VirtualDevice>>) -> Self {
        Self { out, players: HashMap::new() }
    }

    /// Reacts to the bound key `id` being pressed or let go.
    fn handle(&mut self, id: u16, steps: &Arc<Vec<Step>>, mode: Mode, value: i32) {
        let (playing, stop) = self
            .players
            .entry(id)
            .or_insert_with(|| (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false))))
            .clone();
        let is_playing = playing.load(Ordering::SeqCst);
        match (mode, value) {
            (Mode::Once, PRESSED) if !is_playing => self.play(steps, false, playing, stop),
            (Mode::Repeat, PRESSED) if !is_playing => self.play(steps, true, playing, stop),
            (Mode::Repeat, RELEASED) => stop.store(true, Ordering::SeqCst),
            (Mode::Toggle, PRESSED) if is_playing => stop.store(true, Ordering::SeqCst),
            (Mode::Toggle, PRESSED) => self.play(steps, true, playing, stop),
            _ => {}
        }
    }

    fn play(&self, steps: &Arc<Vec<Step>>, looping: bool, playing: Arc<AtomicBool>, stop: Arc<AtomicBool>) {
        playing.store(true, Ordering::SeqCst);
        stop.store(false, Ordering::SeqCst);
        let (steps, out) = (steps.clone(), self.out.clone());
        std::thread::spawn(move || {
            let send = |code: u16, v: i32| {
                if let Ok(mut o) = out.lock() {
                    let _ = o.emit(&[InputEvent::new(EventType::KEY, code, v)]);
                }
            };
            let mut held: HashSet<u16> = HashSet::new();
            'outer: loop {
                for s in steps.iter() {
                    if stop.load(Ordering::SeqCst) {
                        break 'outer;
                    }
                    match *s {
                        Step::Down(k) => {
                            send(k, PRESSED);
                            held.insert(k);
                        }
                        Step::Up(k) => {
                            send(k, RELEASED);
                            held.remove(&k);
                        }
                        Step::Wait(ms) => {
                            // in small slices, so stopping is quick
                            let end = std::time::Instant::now() + std::time::Duration::from_millis(ms as u64);
                            while std::time::Instant::now() < end {
                                if stop.load(Ordering::SeqCst) {
                                    break 'outer;
                                }
                                std::thread::sleep(std::time::Duration::from_millis(5).min(end - std::time::Instant::now()));
                            }
                        }
                    }
                }
                if !looping || stop.load(Ordering::SeqCst) {
                    break;
                }
                // a short breath between repeats, so a macro with no waits
                // can't flood the computer
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            // Never leave a key stuck down.
            for k in held {
                send(k, RELEASED);
            }
            playing.store(false, Ordering::SeqCst);
        });
    }

    fn stop_all(&self) {
        for (_, stop) in self.players.values() {
            stop.store(true, Ordering::SeqCst);
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
        Some(Action::Disabled) | Some(Action::Launch(_)) | Some(Action::Notify(_)) | Some(Action::Macro { .. }) => vec![],
        Some(Action::Combo(keys)) => match value {
            PRESSED => keys.iter().map(|&k| (k, PRESSED)).collect(),
            RELEASED => keys.iter().rev().map(|&k| (k, RELEASED)).collect(),
            _ => vec![],
        },
    }
}

/// Performs actions for presses that don't come from a Linux input device
/// (like Nightsword buttons reported on its own channel), through its own
/// virtual device.
pub struct ActionSender {
    out: Arc<Mutex<VirtualDevice>>,
    notifier: Option<Notifier>,
    macros: Macros,
}

impl ActionSender {
    /// A virtual device able to send every key the given actions use.
    pub fn new(actions: &[Action], notifier: Option<Notifier>) -> std::io::Result<Self> {
        let mut keys = AttributeSet::<Key>::new();
        actions.iter().flat_map(Action::keys).for_each(|k| keys.insert(Key::new(k)));
        // A device with no keys at all isn't allowed; give it one harmless key.
        if keys.iter().next().is_none() {
            keys.insert(Key::KEY_UNKNOWN);
        }
        let out = Arc::new(Mutex::new(VirtualDeviceBuilder::new()?.name(VIRTUAL_NAME).with_keys(&keys)?.build()?));
        Ok(Self { macros: Macros::new(out.clone()), out, notifier })
    }

    /// Sends what `action` does when its button is pressed (`down`) or let
    /// go. `id` tells buttons apart (for macros that keep playing).
    pub fn act(&mut self, id: u16, action: &Action, down: bool) {
        if let Action::Macro { steps, mode } = action {
            self.macros.handle(id, steps, *mode, if down { PRESSED } else { RELEASED });
            return;
        }
        if let (Action::Launch(app), true) = (action, down) {
            launch(app);
        }
        // Reuse the same press/release rules as remapped keys.
        let value = if down { PRESSED } else { RELEASED };
        notify(&self.notifier, Some(action), value);
        let rules: HashMap<u16, Action> = [(0, action.clone())].into();
        let events: Vec<InputEvent> = translate(0, value, &rules)
            .into_iter()
            .map(|(code, v)| InputEvent::new(EventType::KEY, code, v))
            .collect();
        if !events.is_empty() {
            if let Ok(mut o) = self.out.lock() {
                let _ = o.emit(&events);
            }
        }
    }
}

impl Drop for ActionSender {
    fn drop(&mut self) {
        self.macros.stop_all();
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
    pub fn start(bindings: &[Binding], notifier: Option<Notifier>) -> std::io::Result<Self> {
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
            let (stop, out, n) = (stop.clone(), out.clone(), notifier.clone());
            let product = d.product;
            threads.push(std::thread::spawn(move || pump(d.device, product, map, out, stop, n)));
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
fn pump(
    mut dev: Device,
    product: u16,
    bindings: HashMap<u16, Action>,
    out: Arc<Mutex<VirtualDevice>>,
    stop: Arc<AtomicBool>,
    notifier: Option<Notifier>,
) {
    let fd = dev.as_raw_fd();
    let mut macros = Macros::new(out.clone());
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
                    if ev.value() == PRESSED {
                        pressed(product, ev.code());
                    }
                    if let (Some(Action::Launch(app)), PRESSED) = (bindings.get(&ev.code()), ev.value()) {
                        launch(app);
                    }
                    notify(&notifier, bindings.get(&ev.code()), ev.value());
                    if let Some(Action::Macro { steps, mode }) = bindings.get(&ev.code()) {
                        macros.handle(ev.code(), steps, *mode, ev.value());
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
    macros.stop_all();
    let _ = dev.ungrab();
}

/* ---------- presses for lighting ---------- */

/// Told (device's USB product ID, key code) for every key or button press
/// on a Corsair device, for lighting that reacts to typing. Nothing is
/// stored: it only learns where to start an effect.
pub type PressHook = Arc<dyn Fn(u16, u16) + Send + Sync>;

static PRESS_HOOK: Mutex<Option<PressHook>> = Mutex::new(None);

/// Sets (or clears) who hears about presses.
pub fn set_press_hook(hook: Option<PressHook>) {
    if let Ok(mut h) = PRESS_HOOK.lock() {
        *h = hook;
    }
}

fn pressed(product: u16, code: u16) {
    let hook = PRESS_HOOK.lock().ok().and_then(|h| h.clone());
    if let Some(h) = hook {
        h(product, code);
    }
}

/// Watches the Corsair devices for presses without taking them over (so
/// they keep working as normal) and passes them to the press hook. Devices
/// the [`Remapper`] has taken over report their presses from there instead;
/// a taken-over device sends this watcher nothing, so nothing is doubled.
pub struct PressWatcher {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl PressWatcher {
    pub fn start() -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::spawn(move || {
            let mut devices: Vec<(PathBuf, u16, Device)> = Vec::new();
            let mut last_scan: Option<std::time::Instant> = None;
            while !flag.load(Ordering::SeqCst) {
                // Pick up devices plugged in later.
                if last_scan.map_or(true, |t| t.elapsed() > std::time::Duration::from_secs(5)) {
                    last_scan = Some(std::time::Instant::now());
                    for d in corsair_devices().into_iter().filter(|d| d.has_keys()) {
                        if !devices.iter().any(|(p, _, _)| *p == d.path) {
                            devices.push((d.path, d.product, d.device));
                        }
                    }
                }
                let mut gone = Vec::new();
                for (i, (_, product, dev)) in devices.iter_mut().enumerate() {
                    if !readable(dev.as_raw_fd(), if i == 0 { 50 } else { 0 }) {
                        continue;
                    }
                    match dev.fetch_events() {
                        Ok(events) => {
                            for ev in events {
                                if ev.event_type() == EventType::KEY && ev.value() == PRESSED {
                                    pressed(*product, ev.code());
                                }
                            }
                        }
                        Err(_) => gone.push(i), // unplugged
                    }
                }
                for i in gone.into_iter().rev() {
                    devices.remove(i);
                }
                if devices.is_empty() {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
        });
        Self { stop, thread: Some(thread) }
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for PressWatcher {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Records key and button presses from the Corsair devices into macro steps,
/// with the time between them. Nothing is taken over, and the left mouse
/// button is never recorded (so clicking Stop doesn't end up in the macro).
pub struct Recorder {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<Vec<Step>>>,
}

pub const MAX_RECORDED_STEPS: usize = 500;

impl Recorder {
    /// Starts recording; stops by itself after `max` or 500 steps.
    pub fn start(max: std::time::Duration) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::spawn(move || {
            let mut devs: Vec<Device> =
                corsair_devices().into_iter().filter(|d| d.has_keys()).map(|d| d.device).collect();
            let started = std::time::Instant::now();
            let mut last: Option<std::time::Instant> = None;
            let mut steps: Vec<Step> = Vec::new();
            while !flag.load(Ordering::SeqCst) && started.elapsed() < max && steps.len() < MAX_RECORDED_STEPS {
                for d in &mut devs {
                    if !readable(d.as_raw_fd(), 10) {
                        continue;
                    }
                    let Ok(events) = d.fetch_events() else { continue };
                    for ev in events {
                        if ev.event_type() != EventType::KEY || ev.code() == 0x110 || ev.value() > PRESSED {
                            continue; // not a key, the left button, or a key repeat
                        }
                        let now = std::time::Instant::now();
                        if let Some(prev) = last {
                            let ms = now.duration_since(prev).as_millis().min(10_000) as u32;
                            if ms > 0 {
                                steps.push(Step::Wait(ms));
                            }
                        }
                        last = Some(now);
                        steps.push(if ev.value() == PRESSED { Step::Down(ev.code()) } else { Step::Up(ev.code()) });
                    }
                }
            }
            steps.truncate(MAX_RECORDED_STEPS);
            steps
        });
        Self { stop, thread: Some(thread) }
    }

    /// Stops and returns what was recorded.
    pub fn stop(mut self) -> Vec<Step> {
        self.stop.store(true, Ordering::SeqCst);
        self.thread.take().and_then(|t| t.join().ok()).unwrap_or_default()
    }
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
