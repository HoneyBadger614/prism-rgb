//! Bindings for the Nightsword buttons the mouse normally handles itself
//! (DPI, Sniper, Profile). The lighting engine tells the mouse to report
//! those buttons; this listens on the mouse's button channel and performs
//! their actions.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use hidapi::{HidApi, HidDevice};
use prism_core::nightsword::{buttons_held, Nightsword};
use prism_input::{Action, ActionSender};

const RECONNECT_EVERY: Duration = Duration::from_secs(2);

pub struct ButtonListener {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ButtonListener {
    /// Listens for the given buttons (by name) and performs their actions.
    pub fn start(bindings: Vec<(&'static str, Action)>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::spawn(move || {
            let actions: Vec<Action> = bindings.iter().map(|(_, a)| a.clone()).collect();
            let Ok(mut sender) = ActionSender::new(&actions) else {
                eprintln!("Prism: couldn't create the virtual device for mouse buttons");
                return;
            };
            let mut channel: Option<HidDevice> = None;
            let mut last_try: Option<Instant> = None;
            let mut held: HashSet<&'static str> = HashSet::new();
            let mut buf = [0u8; 64];

            while !flag.load(Ordering::SeqCst) {
                let Some(ch) = channel.as_ref() else {
                    if last_try.map_or(true, |t| t.elapsed() >= RECONNECT_EVERY) {
                        last_try = Some(Instant::now());
                        channel = HidApi::new().ok().and_then(|api| Nightsword::open_buttons(&api).ok());
                    }
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                };
                let n = match ch.read_timeout(&mut buf, 100) {
                    Ok(n) => n,
                    Err(_) => {
                        channel = None; // unplugged
                        continue;
                    }
                };
                let Some(now) = (n > 0).then(|| buttons_held(&buf[..n])).flatten() else { continue };
                let now: HashSet<&'static str> = now.into_iter().collect();
                for (name, action) in &bindings {
                    match (held.contains(name), now.contains(name)) {
                        (false, true) => sender.act(action, true),
                        (true, false) => sender.act(action, false),
                        _ => {}
                    }
                }
                held = now;
            }
            // Let go of anything still held, so no key is left stuck down.
            for (name, action) in &bindings {
                if held.contains(name) {
                    sender.act(action, false);
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

impl Drop for ButtonListener {
    fn drop(&mut self) {
        self.shutdown();
    }
}
