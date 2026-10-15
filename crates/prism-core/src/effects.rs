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
}

impl Effect {
    /// Whether the colors change over time (and so need constant updates).
    pub fn is_animated(self) -> bool {
        !matches!(self, Effect::Static | Effect::Gradient)
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
}

fn default_device_layers() -> Vec<Layer> {
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
}

/// What's plugged into one channel: a chain of identical fans. Each fan has
/// an inner ring of LEDs around the hub and an outer ring, wired inner first.
#[derive(Debug, Clone, Serialize)]
pub struct ChannelLayout {
    pub fans: usize,
    /// LEDs around the center hub.
    pub inner: usize,
    /// LEDs on the outer ring.
    pub outer: usize,
}

impl ChannelLayout {
    /// Corsair LL-style fans: 4 LEDs in the hub, 12 on the ring.
    pub const fn ll_fans(fans: usize) -> Self {
        Self { fans, inner: 4, outer: 12 }
    }

    pub fn leds_per_fan(&self) -> usize {
        self.inner + self.outer
    }

    pub fn leds(&self) -> usize {
        self.fans * self.leds_per_fan()
    }
}

/// Every hub LED (`inner = true`) or every outer-ring LED.
pub fn ring_leds(layout: &[ChannelLayout], inner: bool) -> Vec<LedRef> {
    let mut out = Vec::new();
    for (c, ch) in layout.iter().enumerate() {
        for i in 0..ch.leds() {
            if (i % ch.leds_per_fan() < ch.inner) == inner {
                out.push(LedRef(c as u8, i as u16));
            }
        }
    }
    out
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
    /// Fans on a Lighting Node Pro.
    pub fn fans(layout: &[ChannelLayout]) -> Self {
        let total_fans: usize = layout.iter().map(|c| c.fans).sum();
        let mut leds = Vec::new();
        let mut first_fan = 0;
        for ch in layout {
            let per = ch.leds_per_fan().max(1);
            for i in 0..ch.leds() {
                let j = i % per;
                let (k, ring_len) = if j < ch.inner { (j, ch.inner) } else { (j - ch.inner, ch.outer) };
                let fan = first_fan + i / per;
                let around = k as f64 / ring_len.max(1) as f64;
                // across: reach color 2 exactly on the last fan's last LED
                let steps = (total_fans.max(1) * ring_len) as f64 - 1.0;
                let across = if steps > 0.0 { (fan * ring_len + k) as f64 / steps } else { 0.0 };
                leds.push(LedGeom {
                    lin: [across, there_and_back(around)],
                    cyc: [(fan as f64 + around) / total_fans.max(1) as f64, around],
                });
            }
            first_fan += ch.fans;
        }
        Self { channels: layout.iter().map(ChannelLayout::leds).collect(), leds, laps: [total_fans.max(1) as f64, 1.0] }
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
        };
        Self { active: 0, brightness: 100, profiles: vec![profile] }
    }

    /// Fixes anything out of range: numbers, names, too many profiles or
    /// layers, and LEDs that don't exist on the fans or keyboard.
    pub fn sanitized(mut self, fans: &Surface, keyboard: &Surface, mouse: &Surface) -> Self {
        self.brightness = self.brightness.min(100);
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
            p.layers.iter().chain(&p.keyboard).chain(&p.mouse).any(|l| l.enabled && l.effect.is_animated())
        })
    }
}

/// Every LED's color at `t` seconds, one list per channel of `surface`.
/// `master` is the overall brightness, 0–100.
pub fn render_layers(layers: &[Layer], surface: &Surface, master: u8, t: f64) -> Vec<Vec<Rgb>> {
    let total = surface.leds.len();
    let mut flat = vec![BLACK; total];

    for layer in layers.iter().filter(|l| l.enabled) {
        let level = layer.brightness as f32 / 100.0;
        let mut paint = |i: usize| flat[i] = layer_color(layer, &surface.leds[i], surface.laps, t).scale(level);
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

fn layer_color(l: &Layer, g: &LedGeom, laps: [f64; 2], t: f64) -> Rgb {
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
        LightingStore { active: 0, brightness: 100, profiles: vec![Profile { name: "T".into(), layers: vec![layer], keyboard: vec![], mouse: vec![] }] }
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
            profiles: vec![Profile { name: "Hub & Ring".into(), layers: vec![rings, hubs], keyboard: vec![], mouse: vec![] }],
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
            profiles: vec![Profile { name: "x".repeat(100), layers: vec![layer; MAX_LAYERS + 5], keyboard: vec![], mouse: vec![] }],
        }
        .sanitized(&Surface::fans(&layout), &Surface::grid(&[(0.0, 0.0)]), &Surface::grid(&[(0.0, 0.0)]));
        assert_eq!((s.active, s.brightness), (0, 100));
        assert_eq!(s.profiles[0].name.chars().count(), MAX_NAME_LEN);
        assert_eq!(s.profiles[0].layers.len(), MAX_LAYERS);
        let l = &s.profiles[0].layers[0];
        assert_eq!((l.brightness, l.speed, l.name.as_str()), (100, 1, "Layer"));
        assert_eq!(l.target, Target::Leds { leds: vec![LedRef(0, 47)] });

        let empty = LightingStore { active: 0, brightness: 100, profiles: vec![] }
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
