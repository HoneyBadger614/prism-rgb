//! Keeps the input layer's remapping in step with the active profile's
//! bindings.

use std::sync::Mutex;
use std::time::Duration;

use prism_core::bindings::{ActionDef, BindingDef, InputDevice};
use prism_core::effects::LightingStore;
use prism_core::{k65_mini, nightsword};
use prism_input::{capture_press, Action, Binding, Remapper};

pub fn product(device: InputDevice) -> u16 {
    match device {
        InputDevice::Keyboard => k65_mini::PID,
        InputDevice::Mouse => nightsword::PID,
    }
}

fn convert(b: &BindingDef) -> Binding {
    let action = match &b.action {
        ActionDef::Key { key } => Action::Key(*key),
        ActionDef::Combo { keys } => Action::Combo(keys.clone()),
        ActionDef::Launch { app } => Action::Launch(app.clone()),
        ActionDef::Disabled => Action::Disabled,
    };
    Binding { product: product(b.device), input: b.input, action }
}

#[derive(Default)]
struct Inner {
    /// What's running now: (bindings switched on, the bindings).
    applied: Option<(bool, Vec<BindingDef>)>,
    remapper: Option<Remapper>,
    error: Option<String>,
}

#[derive(Default)]
pub struct Remap {
    inner: Mutex<Inner>,
}

impl Remap {
    /// Starts, restarts or stops remapping to match the active profile.
    pub fn apply(&self, store: &LightingStore) {
        let bindings = store.active_profile().map(|p| p.bindings.clone()).unwrap_or_default();
        let wanted = (store.bindings_on, bindings);
        let mut g = self.inner.lock().unwrap();
        if g.applied.as_ref() == Some(&wanted) {
            return;
        }
        if let Some(r) = g.remapper.take() {
            r.stop();
        }
        g.error = None;
        if wanted.0 && !wanted.1.is_empty() {
            let list: Vec<Binding> = wanted.1.iter().map(convert).collect();
            match Remapper::start(&list) {
                Ok(r) => g.remapper = Some(r),
                Err(e) => g.error = Some(format!("Couldn't take over the keyboard or mouse: {e}")),
            }
        }
        g.applied = Some(wanted);
    }

    /// Waits for the next press on a device and returns its code. Remapping
    /// pauses meanwhile so the press is seen as it really is.
    pub fn capture(&self, store: &LightingStore, device: InputDevice, timeout: Duration) -> Option<u16> {
        {
            let mut g = self.inner.lock().unwrap();
            if let Some(r) = g.remapper.take() {
                r.stop();
            }
            g.applied = None;
        }
        let code = capture_press(product(device), timeout);
        self.apply(store);
        code
    }

    pub fn error(&self) -> Option<String> {
        self.inner.lock().unwrap().error.clone()
    }

    pub fn stop(&self) {
        let mut g = self.inner.lock().unwrap();
        if let Some(r) = g.remapper.take() {
            r.stop();
        }
    }
}
