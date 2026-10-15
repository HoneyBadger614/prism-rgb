//! Keeps the input layer's remapping in step with the active profile's
//! bindings.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use prism_core::bindings::{ActionDef, BindingDef, InputDevice, MacroStep, PlayMode};
use prism_core::effects::LightingStore;
use prism_core::{k65_mini, nightsword};
use prism_input::{capture_press, Action, Binding, Mode, Recorder, Remapper, Step};

use crate::engine::{dpi_op_id, Engine};
use crate::mouse_buttons::ButtonListener;

pub fn product(device: InputDevice) -> u16 {
    match device {
        InputDevice::Keyboard => k65_mini::PID,
        InputDevice::Mouse => nightsword::PID,
    }
}

fn action(a: &ActionDef) -> Action {
    match a {
        ActionDef::Key { key } => Action::Key(*key),
        ActionDef::Combo { keys } => Action::Combo(keys.clone()),
        ActionDef::Launch { app } => Action::Launch(app.clone()),
        ActionDef::Dpi { op } => Action::Notify(dpi_op_id(*op)),
        ActionDef::Macro { steps, mode } => Action::Macro {
            steps: Arc::new(
                steps
                    .iter()
                    .map(|s| match *s {
                        MacroStep::Down { key } => Step::Down(key),
                        MacroStep::Up { key } => Step::Up(key),
                        MacroStep::Wait { ms } => Step::Wait(ms),
                    })
                    .collect(),
            ),
            mode: match mode {
                PlayMode::Once => Mode::Once,
                PlayMode::Repeat => Mode::Repeat,
                PlayMode::Toggle => Mode::Toggle,
            },
        },
        ActionDef::Disabled => Action::Disabled,
    }
}

/// The mouse button behind a binding, if it's one the mouse reports on its
/// own channel (DPI, Sniper, Profile) rather than through Linux.
fn reported(b: &BindingDef) -> Option<&'static str> {
    (b.device == InputDevice::Mouse).then(|| nightsword::reported_button(b.input)).flatten()
}

#[derive(Default)]
struct Inner {
    /// What's running now: (bindings switched on, the bindings).
    applied: Option<(bool, Vec<BindingDef>)>,
    remapper: Option<Remapper>,
    buttons: Option<ButtonListener>,
    recorder: Option<Recorder>,
    error: Option<String>,
}

#[derive(Default)]
pub struct Remap {
    inner: Mutex<Inner>,
}

impl Remap {
    /// Starts, restarts or stops remapping to match the active profile.
    pub fn apply(&self, store: &LightingStore, engine: &Engine) {
        let bindings = store.active_profile().map(|p| p.bindings.clone()).unwrap_or_default();
        let wanted = (store.bindings_on, bindings);
        let mut g = self.inner.lock().unwrap();
        if g.applied.as_ref() == Some(&wanted) {
            return;
        }
        if let Some(r) = g.remapper.take() {
            r.stop();
        }
        if let Some(b) = g.buttons.take() {
            b.stop();
        }
        g.error = None;
        let active: &[BindingDef] = if wanted.0 { &wanted.1 } else { &[] };

        // Buttons the mouse reports itself go to the button listener...
        let on_channel: Vec<(&'static str, Action)> =
            active.iter().filter_map(|b| reported(b).map(|name| (name, action(&b.action)))).collect();
        engine.set_mouse_reporting(on_channel.iter().map(|(n, _)| *n).collect());
        if !on_channel.is_empty() {
            g.buttons = Some(ButtonListener::start(on_channel, Some(engine.dpi_notifier())));
        }

        // ...everything else is remapped through Linux input.
        let list: Vec<Binding> = active
            .iter()
            .filter(|b| reported(b).is_none())
            .map(|b| Binding { product: product(b.device), input: b.input, action: action(&b.action) })
            .collect();
        if !list.is_empty() {
            match Remapper::start(&list, Some(engine.dpi_notifier())) {
                Ok(r) => g.remapper = Some(r),
                Err(e) => g.error = Some(format!("Couldn't take over the keyboard or mouse: {e}")),
            }
        }
        g.applied = Some(wanted);
    }

    /// Waits for the next press on a device and returns its code. Remapping
    /// pauses meanwhile so the press is seen as it really is.
    pub fn capture(&self, store: &LightingStore, engine: &Engine, device: InputDevice, timeout: Duration) -> Option<u16> {
        {
            let mut g = self.inner.lock().unwrap();
            if let Some(r) = g.remapper.take() {
                r.stop();
            }
            g.applied = None;
        }
        let code = capture_press(product(device), timeout);
        self.apply(store, engine);
        code
    }

    /// Pauses bindings (so keys are recorded as they really are) and starts
    /// recording a macro. Stops by itself after a minute.
    pub fn record_start(&self) {
        let mut g = self.inner.lock().unwrap();
        if let Some(r) = g.remapper.take() {
            r.stop();
        }
        if let Some(b) = g.buttons.take() {
            b.stop();
        }
        g.applied = None;
        if let Some(old) = g.recorder.take() {
            old.stop();
        }
        g.recorder = Some(Recorder::start(Duration::from_secs(60)));
    }

    /// Stops recording, turns bindings back on, and returns the macro.
    pub fn record_stop(&self, store: &LightingStore, engine: &Engine) -> Vec<MacroStep> {
        let recorder = self.inner.lock().unwrap().recorder.take();
        let steps = recorder.map(Recorder::stop).unwrap_or_default();
        self.apply(store, engine);
        steps
            .into_iter()
            .map(|s| match s {
                Step::Down(key) => MacroStep::Down { key },
                Step::Up(key) => MacroStep::Up { key },
                Step::Wait(ms) => MacroStep::Wait { ms },
            })
            .collect()
    }

    pub fn error(&self) -> Option<String> {
        self.inner.lock().unwrap().error.clone()
    }

    pub fn stop(&self) {
        let mut g = self.inner.lock().unwrap();
        if let Some(r) = g.remapper.take() {
            r.stop();
        }
        if let Some(b) = g.buttons.take() {
            b.stop();
        }
        if let Some(r) = g.recorder.take() {
            r.stop();
        }
    }
}
