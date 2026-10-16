//! Lighting effects. [`render`] works out every LED's color for a moment in
//! time; the app calls it many times a second and sends the result to the
//! device. It's pure math, so the preview and the device always agree.
//!
//! The model:
//! - A [`LightingStore`] holds the saved presets ([`Profile`]s) and which one
//!   is active.
//! - A profile is a stack of [`Layer`]s. Each layer runs one effect on a set
//!   of LEDs ([`Target`]). Layers further down the list paint over earlier
//!   ones.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::Rgb;

const BLACK: Rgb = Rgb::new(0, 0, 0);
const PURPLE: Rgb = Rgb::new(0x7c, 0x3a, 0xed);
const HOT_PINK: Rgb = Rgb::new(0xff, 0x2d, 0x95);

pub const MAX_PROFILES: usize = 50;
pub const MAX_LAYERS: usize = 32;
pub const MAX_NAME_LEN: usize = 40;
/// Most extra colors a layer can take turns with.
pub const MAX_PALETTE: usize = 15;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Effect {
    /// Color 1 everywhere.
    Static,
    /// Color 1 fading into color 2.
    Gradient,
    /// The gradient flowing along the LEDs.
    Wave,
    /// Fading in and out, alternating between the two colors.
    Breathe,
    /// Every color of the rainbow, flowing.
    Rainbow,
    /// All LEDs together, slowly moving through every color.
    ColorCycle,
    /// A bright head (color 1) with a fading tail, running over color 2.
    Comet,
    /// Flashing between color 1 and color 2.
    Strobe,
    /// Rings spreading from each key press, over whatever the layers below
    /// show. Each press takes the next color (color 1, then the palette),
    /// or a random one. The rings travel across every device that has this
    /// effect.
    Ripple,
    /// Each pressed key lights up, then fades back to the layers below
    /// (black if none). Only that key. Colors as for [`Effect::Ripple`].
    KeyGlow,
}

impl Effect {
    /// Whether the colors change over time (and so need constant updates).
    pub fn is_animated(self) -> bool {
        // A ripple only moves after a key press; the engine keeps drawing
        // while one is spreading.
        !matches!(self, Effect::Static | Effect::Gradient | Effect::Ripple | Effect::KeyGlow)
    }
}

/// How a gradient, wave, rainbow or comet is laid out over the fans.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Spread {
    /// One long run from the first fan to the last.
    Across,
    /// Each fan gets the full run around its own rings.
    PerFan,
}

/// One LED: (channel, index on that channel). Stored as `[channel, index]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LedRef(pub u8, pub u16);

/// Which LEDs a layer paints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Target {
    All,
    Leds { leds: Vec<LedRef> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layer {
    pub name: String,
    pub enabled: bool,
    pub effect: Effect,
    pub color1: Rgb,
    pub color2: Rgb,
    pub spread: Spread,
    /// 0–100 percent.
    pub brightness: u8,
    /// 1 (slow) to 10 (fast).
    pub speed: u8,
    pub target: Target,
    /// More colors after color 1, taken in turn (ripples).
    pub palette: Vec<Rgb>,
    /// A random bright color each time instead (ripples).
    pub random: bool,
}

impl Default for Layer {
    fn default() -> Self {
        Self {
            name: "Layer".into(),
            enabled: true,
            effect: Effect::Gradient,
            color1: PURPLE,
            color2: HOT_PINK,
            spread: Spread::Across,
            brightness: 100,
            speed: 5,
            target: Target::All,
            palette: vec![],
            random: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    /// Lighting Node Pro layers.
    pub layers: Vec<Layer>,
    /// Keyboard layers. Profiles saved before keyboards were supported get
    /// the Ember fade.
    #[serde(default = "default_device_layers")]
    pub keyboard: Vec<Layer>,
    /// Mouse layers. Older profiles get the Ember fade.
    #[serde(default = "default_device_layers")]
    pub mouse: Vec<Layer>,
    /// Key and button bindings.
    #[serde(default)]
    pub bindings: Vec<crate::bindings::BindingDef>,
    /// Mouse DPI.
    #[serde(default)]
    pub dpi: crate::bindings::DpiSettings,
    /// Graphics card layers (one LED: the card shows one color).
    #[serde(default = "default_device_layers")]
    pub gpu: Vec<Layer>,
    /// Motherboard layers: channel 0 is the built-in lights, then one
    /// channel per ARGB header.
    #[serde(default = "default_device_layers")]
    pub motherboard: Vec<Layer>,
}

pub fn default_device_layers() -> Vec<Layer> {
    vec![Layer { name: "Ember fade".into(), ..Layer::default() }]
}

/// Everything about lighting that gets saved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LightingStore {
    /// Which profile is showing (index into `profiles`).
    pub active: usize,
    /// Master brightness over everything, 0–100 percent.
    pub brightness: u8,
    pub profiles: Vec<Profile>,
    /// Master switch for key and button bindings.
    #[serde(default = "yes")]
    pub bindings_on: bool,
    /// How many LEDs are plugged into each motherboard ARGB header.
    #[serde(default)]
    pub argb_leds: Vec<u16>,
    /// What's plugged into each lighting controller channel (piece ids, in
    /// wiring order). None in settings from before this existed: those get
    /// [`legacy_layout`].
    #[serde(default)]
    pub node_channels: Option<Vec<Vec<String>>>,
}

fn yes() -> bool {
    true
}

/// How a piece's LEDs are arranged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    /// Rings: an inner ring around the hub (if any), then an outer ring.
    Fan,
    /// A line of LEDs.
    Strip,
}

/// Something that plugs into a lighting controller channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PieceKind {
    pub id: &'static str,
    pub name: &'static str,
    /// LEDs around the center (fans), wired first.
    pub inner: usize,
    /// LEDs on the outer ring (fans) or along the strip.
    pub outer: usize,
    pub shape: Shape,
}

impl PieceKind {
    pub fn leds(&self) -> usize {
        self.inner + self.outer
    }
}

const fn fan(id: &'static str, name: &'static str, inner: usize, outer: usize) -> PieceKind {
    PieceKind { id, name, inner, outer, shape: Shape::Fan }
}

const fn strip(id: &'static str, name: &'static str, leds: usize) -> PieceKind {
    PieceKind { id, name, inner: 0, outer: leds, shape: Shape::Strip }
}

/// Everything Prism knows how to lay out, with LED counts from OpenLinkHub's
/// device lists. Only LL fans have been checked on a real device.
pub const PIECES: &[PieceKind] = &[
    fan("ll", "LL RGB fan", 4, 12),
    fan("ql", "QL RGB fan", 0, 34),
    fan("hd", "HD RGB fan", 0, 12),
    fan("ml", "ML PRO RGB fan", 0, 4),
    fan("8led", "8-LED fan (SP RGB Elite, ...)", 0, 8),
    fan("sp", "SP RGB fan (1 LED)", 0, 1),
    strip("strip", "RGB LED strip", 10),
    strip("ls100-250", "LS100 strip 250 mm", 15),
    strip("ls100-350", "LS100 strip 350 mm", 21),
    strip("ls100-450", "LS100 strip 450 mm", 27),
    strip("ls100-1400", "LS100 strip 1.4 m", 84),
    strip("lc100", "LC100 triangle", 9),
    fan("xd7", "XD7 pump/reservoir", 0, 36),
    fan("xd5", "XD5 pump/reservoir", 0, 10),
    fan("xd3", "XD3 pump/reservoir", 0, 16),
    fan("cx7", "XC7 / XC9 CPU block", 0, 16),
    fan("xc5", "XC5 PRO / XC8 PRO CPU block", 0, 8),
    fan("gpu-block", "GPU water block", 0, 16),
];

pub fn piece(id: &str) -> Option<&'static PieceKind> {
    PIECES.iter().find(|p| p.id == id)
}

/// What's plugged into one channel, in wiring order.
#[derive(Debug, Clone, Serialize)]
pub struct ChannelLayout {
    pub pieces: Vec<&'static PieceKind>,
}

impl ChannelLayout {
    /// Corsair LL fans: 4 LEDs in the hub, 12 on the ring.
    pub fn ll_fans(fans: usize) -> Self {
        Self { pieces: vec![&PIECES[0]; fans] }
    }

    pub fn from_ids(ids: &[String]) -> Self {
        Self { pieces: ids.iter().filter_map(|id| piece(id)).collect() }
    }

    pub fn leds(&self) -> usize {
        self.pieces.iter().map(|p| p.leds()).sum()
    }
}

/// Every hub LED (`inner = true`) or every outer-ring LED of every fan.
pub fn ring_leds(layout: &[ChannelLayout], inner: bool) -> Vec<LedRef> {
    let mut out = Vec::new();
    for (c, ch) in layout.iter().enumerate() {
        let mut i = 0;
        for p in &ch.pieces {
            for j in 0..p.leds() {
                if p.shape == Shape::Fan && (j < p.inner) == inner {
                    out.push(LedRef(c as u8, (i + j) as u16));
                }
            }
            i += p.leds();
        }
    }
    out
}

/// The layout Prism used before channels could be set up: three LL fans
/// on channel 1 (the setup it was first built on).
pub fn legacy_layout() -> Vec<ChannelLayout> {
    vec![ChannelLayout::ll_fans(3), ChannelLayout::ll_fans(0)]
}

/// The lighting controller's channels from saved settings.
pub fn node_layout(store: &LightingStore) -> Vec<ChannelLayout> {
    match &store.node_channels {
        Some(chs) => chs.iter().map(|c| ChannelLayout::from_ids(c)).collect(),
        None => legacy_layout(),
    }
}

/// Any LED a lighting controller could have, so changing what's plugged in
/// doesn't throw away selections.
pub fn node_max_surface() -> Surface {
    Surface::strips(&[crate::lighting_node::MAX_LEDS_PER_CHANNEL; crate::lighting_node::CHANNELS as usize])
}

/// Where one LED sits, as effects see it, in two directions:
/// `[0]` is the main one ("across fans", or left to right on a keyboard) and
/// `[1]` the second ("per fan", or top to bottom on a keyboard).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LedGeom {
    /// 0 on the first LED to 1 on the last, for fades that end exactly on color 2.
    pub lin: [f64; 2],
    /// 0 up to (not including) 1, looping, for things that move.
    pub cyc: [f64; 2],
}

/// Every LED on one device: how many on each channel, and where each sits.
#[derive(Debug, Clone)]
pub struct Surface {
    pub channels: Vec<usize>,
    /// All channels' LEDs, in order.
    pub leds: Vec<LedGeom>,
    /// How many times a comet goes round in each direction before starting
    /// over (once per fan when going across fans).
    pub laps: [f64; 2],
}

/// 0 → 0, 0.5 → 1, 1 → 0. Makes a fade that loops without a visible seam.
fn there_and_back(u: f64) -> f64 {
    1.0 - (2.0 * u - 1.0).abs()
}

impl Surface {
    /// Fans and strips on a lighting controller. "Across" runs over every
    /// piece in turn; the second direction goes round each fan (or along
    /// each strip) on its own.
    pub fn fans(layout: &[ChannelLayout]) -> Self {
        let total: usize = layout.iter().map(|c| c.pieces.len()).sum();
        let total_f = total.max(1) as f64;
        let mut leds = Vec::new();
        let mut n = 0;
        for ch in layout {
            for p in &ch.pieces {
                for j in 0..p.leds() {
                    let (k, len) = if j < p.inner { (j, p.inner) } else { (j - p.inner, p.outer) };
                    let geom = match p.shape {
                        Shape::Fan => {
                            let around = k as f64 / len.max(1) as f64;
                            // reach color 2 exactly on the last piece's last LED
                            let end = if len > 1 { k as f64 / (len - 1) as f64 } else { 1.0 };
                            LedGeom { lin: [(n as f64 + end) / total_f, there_and_back(around)], cyc: [(n as f64 + around) / total_f, around] }
                        }
                        Shape::Strip => {
                            let along = if len > 1 { k as f64 / (len - 1) as f64 } else { 0.0 };
                            let cyc = k as f64 / len.max(1) as f64;
                            LedGeom { lin: [(n as f64 + along) / total_f, along], cyc: [(n as f64 + cyc) / total_f, cyc] }
                        }
                    };
                    leds.push(geom);
                }
                n += 1;
            }
        }
        // The very first LED starts the fade.
        if let Some(first) = leds.first_mut() {
            first.lin[0] = 0.0;
        }
        Self { channels: layout.iter().map(ChannelLayout::leds).collect(), leds, laps: [total_f, 1.0] }
    }

    /// LEDs on a flat grid, like keys on a keyboard. `points` are (x, y)
    /// centers in any units; one channel.
    pub fn grid(points: &[(f64, f64)]) -> Self {
        let range = |f: fn(&(f64, f64)) -> f64| {
            let lo = points.iter().map(f).fold(f64::INFINITY, f64::min);
            let hi = points.iter().map(f).fold(f64::NEG_INFINITY, f64::max);
            (lo, (hi - lo).max(f64::EPSILON))
        };
        let (x0, xw) = range(|p| p.0);
        let (y0, yh) = range(|p| p.1);
        let leds = points
            .iter()
            .map(|&(x, y)| {
                let u = [(x - x0) / xw, (y - y0) / yh];
                // A little short of 1 so the last LED doesn't wrap onto the first.
                LedGeom { lin: u, cyc: [u[0] * 0.97, u[1] * 0.8] }
            })
            .collect();
        Self { channels: vec![points.len()], leds, laps: [1.0, 1.0] }
    }

    /// LEDs in rows, one channel per row (like a motherboard's built-in
    /// lights and its ARGB headers). "Across" runs over every row in turn;
    /// the second direction runs along each row on its own.
    pub fn strips(lens: &[usize]) -> Self {
        let total: usize = lens.iter().sum();
        let mut leds = Vec::with_capacity(total);
        let mut n = 0;
        for &len in lens {
            for i in 0..len {
                let along = |k: usize, of: usize| if of > 1 { k as f64 / (of - 1) as f64 } else { 0.0 };
                leds.push(LedGeom {
                    lin: [along(n, total), along(i, len)],
                    cyc: [n as f64 / total.max(1) as f64, i as f64 / len.max(1) as f64],
                });
                n += 1;
            }
        }
        Self { channels: lens.to_vec(), leds, laps: [1.0, 1.0] }
    }

    /// Position in `leds` of a (channel, index) reference, if it exists.
    pub fn index(&self, r: LedRef) -> Option<usize> {
        let (c, i) = (r.0 as usize, r.1 as usize);
        let len = *self.channels.get(c)?;
        (i < len).then(|| self.channels[..c].iter().sum::<usize>() + i)
    }
}

fn clean_name(name: &str, fallback: &str) -> String {
    let n: String = name.chars().filter(|c| !c.is_control()).take(MAX_NAME_LEN).collect();
    let n = n.trim();
    if n.is_empty() { fallback.to_string() } else { n.to_string() }
}

impl Layer {
    fn sanitized(mut self, surface: &Surface) -> Self {
        self.name = clean_name(&self.name, "Layer");
        self.brightness = self.brightness.min(100);
        self.speed = self.speed.clamp(1, 10);
        self.palette.truncate(MAX_PALETTE);
        if let Target::Leds { leds } = &mut self.target {
            let set: BTreeSet<LedRef> = leds.iter().copied().filter(|r| surface.index(*r).is_some()).collect();
            *leds = set.into_iter().collect();
        }
        self
    }
}

fn sanitize_layers(mut layers: Vec<Layer>, surface: &Surface) -> Vec<Layer> {
    layers.truncate(MAX_LAYERS);
    layers.into_iter().map(|l| l.sanitized(surface)).collect()
}

impl LightingStore {
    /// What Prism starts with: one profile with the Ember purple-to-pink fade.
    /// (Ready-made looks live in the app's effect picker.)
    pub fn starter() -> Self {
        let profile = Profile {
            name: "My profile".into(),
            layers: vec![Layer { name: "Ember fade".into(), ..Layer::default() }],
            keyboard: default_device_layers(),
            mouse: default_device_layers(),
            bindings: vec![],
            dpi: Default::default(),
            gpu: default_device_layers(),
            motherboard: default_device_layers(),
        };
        Self { active: 0, brightness: 100, profiles: vec![profile], bindings_on: true, argb_leds: vec![], node_channels: Some(vec![vec![], vec![]]) }
    }

    /// Fixes anything out of range: numbers, names, too many profiles or
    /// layers, and LEDs that don't exist on the fans or keyboard.
    pub fn sanitized(mut self, fans: &Surface, keyboard: &Surface, mouse: &Surface) -> Self {
        self.brightness = self.brightness.min(100);
        self.argb_leds.truncate(8);
        if let Some(chs) = &mut self.node_channels {
            chs.truncate(crate::lighting_node::CHANNELS as usize);
            for ch in chs.iter_mut() {
                ch.retain(|id| piece(id).is_some());
                // Drop pieces past what one channel can drive.
                let mut leds = 0;
                ch.retain(|id| {
                    leds += piece(id).map_or(0, |p| p.leds());
                    leds <= crate::lighting_node::MAX_LEDS_PER_CHANNEL
                });
            }
        }
        for n in &mut self.argb_leds {
            *n = (*n).min(crate::asus_aura::MAX_ARGB_LEDS as u16);
        }
        // Any LED a motherboard could have, so changing a header's LED count
        // doesn't throw away selections.
        let board = Surface::strips(&[crate::asus_aura::MAX_ARGB_LEDS; 9]);
        let gpu = Surface::grid(&[(0.0, 0.0)]);
        self.profiles.truncate(MAX_PROFILES);
        if self.profiles.is_empty() {
            return Self::starter();
        }
        self.profiles = self
            .profiles
            .into_iter()
            .map(|p| Profile {
                name: clean_name(&p.name, "Profile"),
                layers: sanitize_layers(p.layers, fans),
                keyboard: sanitize_layers(p.keyboard, keyboard),
                mouse: sanitize_layers(p.mouse, mouse),
                bindings: crate::bindings::sanitize(p.bindings),
                dpi: p.dpi.sanitized(),
                gpu: sanitize_layers(p.gpu, &gpu),
                motherboard: sanitize_layers(p.motherboard, &board),
            })
            .collect();
        self.active = self.active.min(self.profiles.len() - 1);
        self
    }

    pub fn active_profile(&self) -> Option<&Profile> {
        self.profiles.get(self.active)
    }

    /// Whether the active profile changes over time (and so needs constant updates).
    pub fn is_animated(&self) -> bool {
        self.active_profile().is_some_and(|p| {
            p.layers.iter().chain(&p.keyboard).chain(&p.mouse).chain(&p.gpu).chain(&p.motherboard).any(|l| l.enabled && l.effect.is_animated())
        })
    }
}

/// What one LED can show by itself when saved to a device's memory.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OnboardLight {
    /// A steady color (a still picture of the LED).
    Steady(Rgb),
    /// The device's own rainbow, for the layer at this index.
    Rainbow(usize),
    /// The device's own pulse between two colors, for the layer at this index.
    Pulse(usize, Rgb, Rgb),
}

/// For saving to a device: each LED gets the effect of the topmost layer
/// over it if the device can play it itself (rainbow, color cycle,
/// breathe), otherwise its color at `t` seconds.
pub fn onboard_lights(layers: &[Layer], surface: &Surface, master: u8, t: f64) -> Vec<OnboardLight> {
    let still: Vec<Rgb> = render_layers(layers, surface, master, t).concat();
    let mut top: Vec<Option<usize>> = vec![None; surface.leds.len()];
    // Ripples can't play without Prism, so they don't count.
    for (n, layer) in layers.iter().enumerate().filter(|(_, l)| l.enabled && !matches!(l.effect, Effect::Ripple | Effect::KeyGlow)) {
        match &layer.target {
            Target::All => top.iter_mut().for_each(|t| *t = Some(n)),
            Target::Leds { leds } => leds.iter().filter_map(|r| surface.index(*r)).for_each(|i| top[i] = Some(n)),
        }
    }
    let master = master as f32 / 100.0;
    top.iter()
        .enumerate()
        .map(|(i, t)| match t.map(|n| (n, &layers[n])) {
            Some((n, l)) if matches!(l.effect, Effect::Rainbow | Effect::ColorCycle) => OnboardLight::Rainbow(n),
            Some((n, l)) if l.effect == Effect::Breathe => {
                let level = l.brightness as f32 / 100.0 * master;
                OnboardLight::Pulse(n, l.color1.scale(level), l.color2.scale(level))
            }
            _ => OnboardLight::Steady(still[i]),
        })
        .collect()
    // (A ripple layer saves as its background color: it can't react to
    // typing without Prism.)
}

/// One key press spreading out: when (in effect seconds) and where, on the
/// desk every device sits on (0-1 across, 0-1 front to back).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ripple {
    pub t0: f64,
    pub x: f64,
    pub y: f64,
    /// Which press this was (0, 1, 2...), so every device gives it the same color.
    pub n: u64,
    /// Only for key glows, not rings (the other lights of a key with
    /// several, like the space bar).
    pub glow_only: bool,
}

impl Layer {
    /// The color a ripple layer gives press number `n`.
    pub fn ripple_color(&self, n: u64) -> Rgb {
        if self.random {
            // A well-spread hue per press (golden-ratio steps), always bright.
            let h = (n as f64 * 0.618_033_988_75).fract() * 360.0;
            return Rgb::from_hsv(h as f32, 1.0, 1.0);
        }
        let count = 1 + self.palette.len() as u64;
        match n % count {
            0 => self.color1,
            i => self.palette[i as usize - 1],
        }
    }
}

/// Where a device sits on the desk ([x, y, width, height]) and the ripples
/// spreading over it, for effects that cross devices.
#[derive(Debug, Clone, Copy)]
pub struct World<'a> {
    pub ripples: &'a [Ripple],
    pub place: [f64; 4],
}

/// Where each kind of device sits on the desk: the keyboard in front, the
/// mouse to its right, the PC behind on the left.
pub mod desk {
    pub const KEYBOARD: [f64; 4] = [0.30, 0.62, 0.40, 0.16];
    pub const MOUSE: [f64; 4] = [0.78, 0.62, 0.06, 0.16];
    pub const FANS: [f64; 4] = [0.02, 0.05, 0.20, 0.22];
    pub const MOTHERBOARD: [f64; 4] = [0.06, 0.30, 0.12, 0.12];
    pub const GPU: [f64; 4] = [0.04, 0.45, 0.16, 0.05];
}

/// Rings this wide (in desk units) at their brightest.
const RIPPLE_WIDTH: f64 = 0.05;
/// A ripple fades out by the time it's this far from where it started.
pub const RIPPLE_REACH: f64 = 1.4;

/// How fast ripples spread for a layer `speed` of 1-10, in desk units per second.
fn ripple_speed(speed: u8) -> f64 {
    0.12 * speed as f64
}

/// A key glow lights LEDs this close to the press (in desk units): the key
/// itself, not its neighbours.
const GLOW_RADIUS: f64 = 0.01;

/// How long a key glow takes to fade, in seconds, for a layer `speed` of 1-10.
fn glow_fade(speed: u8) -> f64 {
    2.2 - 0.2 * speed as f64
}

/// Whether a ripple started at `t0` is still spreading at `t`, at the
/// fastest speed.
pub fn ripple_alive(r: &Ripple, t: f64) -> bool {
    (t - r.t0) * ripple_speed(1) < RIPPLE_REACH
}

/// Every LED's color at `t` seconds, one list per channel of `surface`.
/// `master` is the overall brightness, 0–100.
pub fn render_layers(layers: &[Layer], surface: &Surface, master: u8, t: f64) -> Vec<Vec<Rgb>> {
    render_layers_in(layers, surface, master, t, None)
}

/// Like [`render_layers`], with key-press ripples crossing the desk.
pub fn render_layers_in(layers: &[Layer], surface: &Surface, master: u8, t: f64, world: Option<&World>) -> Vec<Vec<Rgb>> {
    let total = surface.leds.len();
    let mut flat = vec![BLACK; total];

    for layer in layers.iter().filter(|l| l.enabled) {
        let level = layer.brightness as f32 / 100.0;
        if matches!(layer.effect, Effect::Ripple | Effect::KeyGlow) {
            // Only the rings (or glowing keys) are painted, over what the
            // layers below show.
            let Some(w) = world else { continue };
            let mut ring = |i: usize| {
                let found = if layer.effect == Effect::Ripple { ripple_at(layer, &surface.leds[i], t, w) } else { glow_at(layer, &surface.leds[i], t, w) };
                if let Some((c, glow)) = found {
                    flat[i] = flat[i].mix(c, glow as f32 * level);
                }
            };
            match &layer.target {
                Target::All => (0..total).for_each(&mut ring),
                Target::Leds { leds } => leds.iter().filter_map(|r| surface.index(*r)).for_each(&mut ring),
            }
            continue;
        }
        let mut paint = |i: usize| flat[i] = layer_color(layer, &surface.leds[i], surface.laps, t, world).scale(level);
        match &layer.target {
            Target::All => (0..total).for_each(&mut paint),
            Target::Leds { leds } => leds.iter().filter_map(|r| surface.index(*r)).for_each(&mut paint),
        }
    }

    let master = master as f32 / 100.0;
    let mut out = Vec::with_capacity(surface.channels.len());
    let mut rest = flat.as_slice();
    for &n in &surface.channels {
        let (ch, tail) = rest.split_at(n);
        out.push(ch.iter().map(|c| c.scale(master)).collect());
        rest = tail;
    }
    out
}

/// The fans' colors for the active profile at `t` seconds.
pub fn render(store: &LightingStore, layout: &[ChannelLayout], t: f64) -> Vec<Vec<Rgb>> {
    let surface = Surface::fans(layout);
    match store.active_profile() {
        Some(p) => render_layers(&p.layers, &surface, store.brightness, t),
        None => surface.channels.iter().map(|&n| vec![BLACK; n]).collect(),
    }
}

/// The brightest ripple ring over one LED: its color and how strongly
/// (0-1). None if no ring is there.
fn ripple_at(l: &Layer, g: &LedGeom, t: f64, w: &World) -> Option<(Rgb, f64)> {
    let [px, py, pw, ph] = w.place;
    let (x, y) = (px + g.lin[0] * pw, py + g.lin[1] * ph);
    let v = ripple_speed(l.speed);
    w.ripples
        .iter()
        .filter(|r| t >= r.t0 && !r.glow_only)
        .map(|r| {
            let radius = (t - r.t0) * v;
            let d = ((x - r.x).powi(2) + (y - r.y).powi(2)).sqrt();
            let ring = (1.0 - (d - radius).abs() / RIPPLE_WIDTH).max(0.0);
            (r, ring * (1.0 - radius / RIPPLE_REACH).max(0.0))
        })
        .filter(|(_, glow)| *glow > 0.0)
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(r, glow)| (l.ripple_color(r.n), glow))
}

/// The brightest key glow on one LED: its color and how strongly (0-1).
fn glow_at(l: &Layer, g: &LedGeom, t: f64, w: &World) -> Option<(Rgb, f64)> {
    let [px, py, pw, ph] = w.place;
    let (x, y) = (px + g.lin[0] * pw, py + g.lin[1] * ph);
    let fade = glow_fade(l.speed);
    w.ripples
        .iter()
        .filter(|r| t >= r.t0 && ((x - r.x).powi(2) + (y - r.y).powi(2)).sqrt() < GLOW_RADIUS)
        .map(|r| (r, (1.0 - (t - r.t0) / fade).max(0.0)))
        .filter(|(_, glow)| *glow > 0.0)
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(r, glow)| (l.ripple_color(r.n), glow))
}

fn layer_color(l: &Layer, g: &LedGeom, laps: [f64; 2], t: f64, _world: Option<&World>) -> Rgb {
    let d = match l.spread {
        Spread::Across => 0,
        Spread::PerFan => 1,
    };
    let x = g.cyc[d];
    let speed = l.speed as f64;

    match l.effect {
        Effect::Static => l.color1,
        Effect::Gradient => l.color1.mix(l.color2, g.lin[d] as f32),
        Effect::Wave => {
            let phase = (x - t * speed * 0.06).rem_euclid(1.0);
            l.color1.mix(l.color2, there_and_back(phase) as f32)
        }
        Effect::Breathe => {
            let phase = t * speed * 0.05;
            let c = if (phase.floor() as i64) % 2 == 0 { l.color1 } else { l.color2 };
            let s = (std::f64::consts::PI * phase.fract()).sin();
            c.scale((s * s) as f32)
        }
        Effect::Rainbow => Rgb::from_hsv(((x - t * speed * 0.04).rem_euclid(1.0) * 360.0) as f32, 1.0, 1.0),
        Effect::ColorCycle => Rgb::from_hsv(((t * speed * 0.02).rem_euclid(1.0) * 360.0) as f32, 1.0, 1.0),
        Effect::Comet => {
            // Across fans the head goes round each fan in turn; per fan it
            // circles every fan at once.
            let laps = laps[d];
            let head = (t * speed * 0.12 / laps).rem_euclid(1.0);
            let behind = (head - x).rem_euclid(1.0) * laps; // 0 at the head, in laps
            let tail = (1.0 - behind / 0.4).max(0.0);
            l.color2.mix(l.color1, (tail * tail) as f32)
        }
        Effect::Strobe => {
            if (t * speed * 0.8).rem_euclid(1.0) < 0.25 { l.color1 } else { l.color2 }
        }
        // Drawn over the layers below in render_layers_in, never on its own.
        Effect::Ripple | Effect::KeyGlow => BLACK,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: Rgb = Rgb::new(255, 255, 255);

    fn three_fans() -> Vec<ChannelLayout> {
        vec![ChannelLayout::ll_fans(3), ChannelLayout::ll_fans(0)]
    }

    fn one(layer: Layer) -> LightingStore {
        LightingStore { active: 0, brightness: 100, bindings_on: true, argb_leds: vec![], node_channels: None, profiles: vec![Profile { name: "T".into(), layers: vec![layer], keyboard: vec![], mouse: vec![], bindings: vec![], dpi: Default::default(), gpu: vec![], motherboard: vec![] }] }
    }

    fn fx(effect: Effect, spread: Spread) -> Layer {
        Layer { effect, spread, ..Default::default() }
    }

    #[test]
    fn frame_matches_layout() {
        let f = render(&LightingStore::starter(), &three_fans(), 0.0);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].len(), 48);
        assert!(f[1].is_empty());
    }

    #[test]
    fn static_is_one_color() {
        let f = render(&one(fx(Effect::Static, Spread::Across)), &three_fans(), 3.0);
        assert!(f[0].iter().all(|&c| c == PURPLE));
    }

    #[test]
    fn gradient_across_runs_first_to_last() {
        let f = render(&one(fx(Effect::Gradient, Spread::Across)), &three_fans(), 0.0);
        assert_eq!(f[0][0], PURPLE); // fan 1, first hub LED
        assert_eq!(f[0][4], PURPLE); // fan 1, first ring LED
        assert_eq!(f[0][35], HOT_PINK); // fan 3, last hub LED
        assert_eq!(f[0][47], HOT_PINK); // fan 3, last ring LED
    }

    #[test]
    fn gradient_per_fan_repeats_on_each_fan() {
        let f = render(&one(fx(Effect::Gradient, Spread::PerFan)), &three_fans(), 0.0);
        for fan in 0..3 {
            assert_eq!(f[0][fan * 16], PURPLE); // hub start
            assert_eq!(f[0][fan * 16 + 2], HOT_PINK); // halfway round the hub
            assert_eq!(f[0][fan * 16 + 4], PURPLE); // ring start
            assert_eq!(f[0][fan * 16 + 10], HOT_PINK); // halfway round the ring
        }
    }

    #[test]
    fn animated_effects_move_and_static_ones_dont() {
        let all = [
            Effect::Static, Effect::Gradient, Effect::Wave, Effect::Breathe,
            Effect::Rainbow, Effect::ColorCycle, Effect::Comet, Effect::Strobe,
        ];
        for e in all {
            let s = one(fx(e, Spread::Across));
            let moved = (1..20).any(|i| render(&s, &three_fans(), 0.0) != render(&s, &three_fans(), i as f64 * 0.13));
            assert_eq!(moved, e.is_animated(), "{e:?}");
        }
    }

    #[test]
    fn breathe_goes_dark_between_breaths() {
        let s = one(fx(Effect::Breathe, Spread::Across));
        assert!(render(&s, &three_fans(), 0.0)[0].iter().all(|&x| x == BLACK));
        assert_eq!(render(&s, &three_fans(), 2.0)[0][0], PURPLE);
        assert_eq!(render(&s, &three_fans(), 6.0)[0][0], HOT_PINK);
    }

    #[test]
    fn comet_has_one_bright_head_per_fan() {
        let s = one(Layer { color1: WHITE, color2: BLACK, ..fx(Effect::Comet, Spread::PerFan) });
        let f = render(&s, &three_fans(), 0.7);
        for fan in 0..3 {
            let ring = &f[0][fan * 16 + 4..fan * 16 + 16];
            let bright = ring.iter().filter(|c| c.r > 200).count();
            let dark = ring.iter().filter(|c| c.r == 0).count();
            assert!(bright >= 1 && bright <= 2, "fan {fan}: {ring:?}");
            assert!(dark >= 6, "tail should be short: {ring:?}");
        }
    }

    #[test]
    fn upper_layers_paint_over_lower_ones() {
        let layout = three_fans();
        let rings = Layer { effect: Effect::Wave, spread: Spread::PerFan, ..Default::default() };
        let hubs = Layer {
            effect: Effect::Static,
            color1: WHITE,
            target: Target::Leds { leds: ring_leds(&layout, true) },
            ..Default::default()
        };
        let hub_ring = LightingStore {
            active: 0,
            brightness: 100,
            bindings_on: true,
            argb_leds: vec![], node_channels: None,
            profiles: vec![Profile { name: "Hub & Ring".into(), layers: vec![rings, hubs], keyboard: vec![], mouse: vec![], bindings: vec![], dpi: Default::default(), gpu: vec![], motherboard: vec![] }],
        };
        let f = render(&hub_ring, &layout, 1.0);
        for fan in 0..3 {
            assert!(f[0][fan * 16..fan * 16 + 4].iter().all(|&c| c == WHITE), "hubs white");
            assert!(f[0][fan * 16 + 4..fan * 16 + 16].iter().all(|&c| c != WHITE), "rings not white");
        }
    }

    #[test]
    fn hidden_layers_and_unpainted_leds_stay_dark() {
        let mut layer = fx(Effect::Static, Spread::Across);
        layer.target = Target::Leds { leds: vec![LedRef(0, 5)] };
        let f = render(&one(layer.clone()), &three_fans(), 0.0);
        assert_eq!(f[0][5], PURPLE);
        assert_eq!(f[0].iter().filter(|&&c| c != BLACK).count(), 1);

        layer.enabled = false;
        assert!(render(&one(layer), &three_fans(), 0.0)[0].iter().all(|&c| c == BLACK));
    }

    #[test]
    fn brightness_layers_and_master() {
        let mut s = one(Layer { color1: Rgb::new(200, 100, 0), brightness: 50, ..fx(Effect::Static, Spread::Across) });
        assert_eq!(render(&s, &three_fans(), 0.0)[0][0], Rgb::new(100, 50, 0));
        s.brightness = 50;
        assert_eq!(render(&s, &three_fans(), 0.0)[0][0], Rgb::new(50, 25, 0));
        s.brightness = 0;
        assert!(render(&s, &three_fans(), 0.0)[0].iter().all(|&c| c == BLACK));
    }

    #[test]
    fn sanitize_fixes_bad_input() {
        let layout = three_fans();
        let mut layer = Layer { brightness: 250, speed: 0, name: "  \u{7}  ".into(), ..Default::default() };
        layer.target = Target::Leds { leds: vec![LedRef(0, 47), LedRef(0, 48), LedRef(1, 0), LedRef(9, 0), LedRef(0, 47)] };
        let s = LightingStore {
            active: 99,
            brightness: 200,
            bindings_on: true,
            argb_leds: vec![], node_channels: None,
            profiles: vec![Profile { name: "x".repeat(100), layers: vec![layer; MAX_LAYERS + 5], keyboard: vec![], mouse: vec![], bindings: vec![], dpi: Default::default(), gpu: vec![], motherboard: vec![] }],
        }
        .sanitized(&Surface::fans(&layout), &Surface::grid(&[(0.0, 0.0)]), &Surface::grid(&[(0.0, 0.0)]));
        assert_eq!((s.active, s.brightness), (0, 100));
        assert_eq!(s.profiles[0].name.chars().count(), MAX_NAME_LEN);
        assert_eq!(s.profiles[0].layers.len(), MAX_LAYERS);
        let l = &s.profiles[0].layers[0];
        assert_eq!((l.brightness, l.speed, l.name.as_str()), (100, 1, "Layer"));
        assert_eq!(l.target, Target::Leds { leds: vec![LedRef(0, 47)] });

        let empty = LightingStore { active: 0, brightness: 100, profiles: vec![], bindings_on: true, argb_leds: vec![], node_channels: None }
            .sanitized(&Surface::fans(&layout), &Surface::grid(&[(0.0, 0.0)]), &Surface::grid(&[(0.0, 0.0)]));
        assert!(!empty.profiles.is_empty());
    }

    #[test]
    fn keyboard_gradient_runs_left_to_right_and_top_to_bottom() {
        // a 3 x 2 grid: x = 0, 1, 2 on rows 0 and 1
        let pts = [(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (0.0, 1.0), (1.0, 1.0), (2.0, 1.0)];
        let kb = Surface::grid(&pts);
        let lr = render_layers(&[fx(Effect::Gradient, Spread::Across)], &kb, 100, 0.0);
        assert_eq!((lr[0][0], lr[0][2], lr[0][3]), (PURPLE, HOT_PINK, PURPLE));
        let tb = render_layers(&[fx(Effect::Gradient, Spread::PerFan)], &kb, 100, 0.0);
        assert_eq!((tb[0][0], tb[0][2], tb[0][5]), (PURPLE, PURPLE, HOT_PINK));
    }

    #[test]
    fn old_profiles_get_keyboard_layers() {
        let old = r#"{"active":0,"brightness":80,"profiles":[{"name":"Mine","layers":[]}]}"#;
        let s: LightingStore = serde_json::from_str(old).unwrap();
        assert_eq!(s.profiles[0].keyboard.len(), 1);
        assert_eq!(s.profiles[0].mouse.len(), 1);
    }

    #[test]
    fn strips_run_across_rows_and_along_each() {
        let s = Surface::strips(&[2, 3]);
        let across = render_layers(&[fx(Effect::Gradient, Spread::Across)], &s, 100, 0.0);
        assert_eq!((across[0][0], across[1][2]), (PURPLE, HOT_PINK));
        let along = render_layers(&[fx(Effect::Gradient, Spread::PerFan)], &s, 100, 0.0);
        assert_eq!((along[0][0], along[0][1], along[1][0], along[1][2]), (PURPLE, HOT_PINK, PURPLE, HOT_PINK));
        assert_eq!(Surface::strips(&[5, 0]).channels, vec![5, 0]);
    }

    #[test]
    fn onboard_lights_take_the_top_layer() {
        let kb = Surface::grid(&[(0.0, 0.0), (1.0, 0.0), (2.0, 0.0)]);
        let base = Layer { effect: Effect::Rainbow, ..Default::default() };
        let pulse = Layer { effect: Effect::Breathe, target: Target::Leds { leds: vec![LedRef(0, 1)] }, ..Default::default() };
        let fixed = Layer { effect: Effect::Static, color1: WHITE, target: Target::Leds { leds: vec![LedRef(0, 2)] }, ..Default::default() };
        let l = onboard_lights(&[base, pulse, fixed], &kb, 100, 0.0);
        assert_eq!(l, vec![OnboardLight::Rainbow(0), OnboardLight::Pulse(1, PURPLE, HOT_PINK), OnboardLight::Steady(WHITE)]);
        assert_eq!(onboard_lights(&[], &kb, 100, 0.0)[0], OnboardLight::Steady(BLACK));
    }

    #[test]
    fn mixed_pieces_and_saved_channels() {
        let ch = ChannelLayout::from_ids(&["ql".into(), "strip".into(), "nope".into()]);
        assert_eq!(ch.leds(), 44);
        let s = Surface::fans(&[ch]);
        let f = render_layers(&[fx(Effect::Gradient, Spread::Across)], &s, 100, 0.0);
        assert_eq!((f[0][0], f[0][43]), (PURPLE, HOT_PINK));
        // along the strip on its own: first and last LED
        let along = render_layers(&[fx(Effect::Gradient, Spread::PerFan)], &s, 100, 0.0);
        assert_eq!((along[0][34], along[0][43]), (PURPLE, HOT_PINK));
        // no hub LEDs on a QL or a strip
        assert!(ring_leds(&[ChannelLayout::from_ids(&["ql".into(), "strip".into()])], true).is_empty());

        // Old settings keep the original 3 LL fans; new ones start empty.
        let old: LightingStore = serde_json::from_str(r#"{"active":0,"brightness":80,"profiles":[{"name":"Mine","layers":[]}]}"#).unwrap();
        assert_eq!(node_layout(&old)[0].leds(), 48);
        assert_eq!(node_layout(&LightingStore::starter())[0].leds(), 0);

        // Too many LEDs for a channel: the extra pieces are dropped.
        let mut store = LightingStore::starter();
        store.node_channels = Some(vec![vec!["lc100".into(); 30], vec!["xx".into()], vec!["ll".into()]]);
        let k = Surface::grid(&[(0.0, 0.0)]);
        let store = store.sanitized(&node_max_surface(), &k, &k);
        let chs = store.node_channels.unwrap();
        assert_eq!((chs.len(), chs[0].len(), chs[1].len()), (2, 22, 0));
    }

    #[test]
    fn ripples_spread_over_the_layers_below() {
        let kb = Surface::grid(&[(0.0, 0.0), (1.0, 0.0)]);
        let base = Layer { effect: Effect::Static, color1: PURPLE, ..Default::default() };
        let ripple = Layer { effect: Effect::Ripple, color1: WHITE, palette: vec![HOT_PINK], speed: 5, ..Default::default() };
        // Press 0 at the keyboard's left end: the left key lights first, the
        // right one (0.4 desk units away) once the ring gets there.
        let r = [Ripple { t0: 0.0, x: 0.30, y: 0.62, n: 0, glow_only: false }];
        let w = World { ripples: &r, place: desk::KEYBOARD };
        let at = |t: f64, rs: &World| render_layers_in(&[base.clone(), ripple.clone()], &kb, 100, t, Some(rs))[0].clone();
        assert_eq!(at(0.0, &w), vec![WHITE, PURPLE]); // ring color over the purple below
        let reach = 0.4 / (0.12 * 5.0);
        assert!(at(reach, &w)[1].g > 150, "{:?}", at(reach, &w));
        assert_eq!(at(reach, &w)[0], PURPLE);
        // Press 1 takes the next color.
        let r1 = [Ripple { t0: 0.0, x: 0.30, y: 0.62, n: 1, glow_only: false }];
        assert_eq!(at(0.0, &World { ripples: &r1, place: desk::KEYBOARD })[0], HOT_PINK);
        // Without ripples the layers below show untouched.
        assert!(render_layers(&[base.clone(), ripple.clone()], &kb, 100, 1.0)[0].iter().all(|&c| c == PURPLE));
        assert!(!Effect::Ripple.is_animated());
        assert!(!ripple_alive(&r[0], 20.0));
        // Random colors: bright and different from one press to the next.
        let rnd = Layer { random: true, ..ripple.clone() };
        assert_ne!(rnd.ripple_color(0), rnd.ripple_color(1));
        let c = rnd.ripple_color(7);
        assert_eq!(c.r.max(c.g).max(c.b), 255);
    }

    #[test]
    fn key_glow_lights_only_the_pressed_key_then_fades() {
        let kb = Surface::grid(&[(0.0, 0.0), (1.0, 0.0), (2.0, 0.0)]);
        let glow = Layer { effect: Effect::KeyGlow, color1: WHITE, speed: 5, ..Default::default() };
        // A press exactly on the middle key (desk x = 0.30 + 0.5 * 0.40).
        let r = [Ripple { t0: 0.0, x: 0.50, y: 0.62, n: 0, glow_only: true }];
        let w = World { ripples: &r, place: desk::KEYBOARD };
        let at = |t: f64| render_layers_in(&[glow.clone()], &kb, 100, t, Some(&w))[0].clone();
        assert_eq!(at(0.0), vec![BLACK, WHITE, BLACK]);
        assert!(at(0.6)[1].r > 0 && at(0.6)[1].r < 255);
        assert_eq!(at(1.3), vec![BLACK, BLACK, BLACK]); // faded (1.2 s at speed 5)
        // A glow-only press makes no ring.
        let ring = Layer { effect: Effect::Ripple, color1: WHITE, ..Default::default() };
        assert!(render_layers_in(&[ring], &kb, 100, 0.0, Some(&w))[0].iter().all(|&c| c == BLACK));
        assert!(!Effect::KeyGlow.is_animated());
    }

    #[test]
    fn surface_index_finds_channels() {
        let s = Surface::fans(&[ChannelLayout::ll_fans(1), ChannelLayout::ll_fans(2)]);
        assert_eq!(s.index(LedRef(0, 15)), Some(15));
        assert_eq!(s.index(LedRef(1, 0)), Some(16));
        assert_eq!(s.index(LedRef(1, 32)), None);
        assert_eq!(s.index(LedRef(2, 0)), None);
    }

    #[test]
    fn store_json_round_trip() {
        let s = LightingStore::starter();
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""kind":"all""#));
        assert_eq!(serde_json::from_str::<LightingStore>(&json).unwrap(), s);
        assert!(serde_json::from_str::<LightingStore>(r#"{"effect":"wave"}"#).is_err());
    }
}
