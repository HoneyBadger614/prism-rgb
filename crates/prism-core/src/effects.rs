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
    pub layers: Vec<Layer>,
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

fn clean_name(name: &str, fallback: &str) -> String {
    let n: String = name.chars().filter(|c| !c.is_control()).take(MAX_NAME_LEN).collect();
    let n = n.trim();
    if n.is_empty() { fallback.to_string() } else { n.to_string() }
}

impl Layer {
    fn sanitized(mut self, layout: &[ChannelLayout]) -> Self {
        self.name = clean_name(&self.name, "Layer");
        self.brightness = self.brightness.min(100);
        self.speed = self.speed.clamp(1, 10);
        if let Target::Leds { leds } = &mut self.target {
            let set: BTreeSet<LedRef> = leds
                .iter()
                .copied()
                .filter(|r| layout.get(r.0 as usize).is_some_and(|ch| (r.1 as usize) < ch.leds()))
                .collect();
            *leds = set.into_iter().collect();
        }
        self
    }
}

impl LightingStore {
    /// What Prism starts with: one profile with the Ember purple-to-pink fade.
    /// (Ready-made looks live in the app's effect picker.)
    pub fn starter(_layout: &[ChannelLayout]) -> Self {
        let ember = Layer { name: "Ember fade".into(), ..Layer::default() };
        Self { active: 0, brightness: 100, profiles: vec![Profile { name: "My profile".into(), layers: vec![ember] }] }
    }

    /// Fixes anything out of range: numbers, names, too many presets or
    /// layers, and LEDs that don't exist in `layout`.
    pub fn sanitized(mut self, layout: &[ChannelLayout]) -> Self {
        self.brightness = self.brightness.min(100);
        self.profiles.truncate(MAX_PROFILES);
        if self.profiles.is_empty() {
            return Self::starter(layout);
        }
        self.profiles = self
            .profiles
            .into_iter()
            .map(|mut p| {
                p.name = clean_name(&p.name, "Preset");
                p.layers.truncate(MAX_LAYERS);
                p.layers = p.layers.into_iter().map(|l| l.sanitized(layout)).collect();
                p
            })
            .collect();
        self.active = self.active.min(self.profiles.len() - 1);
        self
    }

    pub fn active_profile(&self) -> Option<&Profile> {
        self.profiles.get(self.active)
    }

    /// Whether the active preset changes over time (and so needs constant updates).
    pub fn is_animated(&self) -> bool {
        self.active_profile()
            .is_some_and(|p| p.layers.iter().any(|l| l.enabled && l.effect.is_animated()))
    }
}

/// Where one LED sits: which fan (counting across all channels), and its
/// place on its own ring.
struct Spot {
    fan: usize,
    k: usize,
    ring_len: usize,
}

fn spot(layout: &[ChannelLayout], channel: usize, index: usize) -> Spot {
    let first_fan: usize = layout[..channel].iter().map(|c| c.fans).sum();
    let ch = &layout[channel];
    let per = ch.leds_per_fan().max(1);
    let j = index % per;
    let (k, ring_len) = if j < ch.inner { (j, ch.inner) } else { (j - ch.inner, ch.outer) };
    Spot { fan: first_fan + index / per, k, ring_len }
}

/// 0 → 0, 0.5 → 1, 1 → 0. Makes a fade that loops without a visible seam.
fn there_and_back(u: f64) -> f64 {
    1.0 - (2.0 * u - 1.0).abs()
}

/// Every LED's color at `t` seconds, one list per channel.
pub fn render(store: &LightingStore, layout: &[ChannelLayout], t: f64) -> Vec<Vec<Rgb>> {
    let mut frame: Vec<Vec<Rgb>> = layout.iter().map(|c| vec![BLACK; c.leds()]).collect();
    let fans: usize = layout.iter().map(|c| c.fans).sum();
    let Some(profile) = store.active_profile() else { return frame };

    for layer in profile.layers.iter().filter(|l| l.enabled) {
        let level = layer.brightness as f32 / 100.0;
        let mut paint = |c: usize, i: usize| {
            frame[c][i] = layer_color(layer, &spot(layout, c, i), fans, t).scale(level);
        };
        match &layer.target {
            Target::All => {
                for (c, ch) in layout.iter().enumerate() {
                    (0..ch.leds()).for_each(|i| paint(c, i));
                }
            }
            Target::Leds { leds } => {
                for r in leds {
                    let (c, i) = (r.0 as usize, r.1 as usize);
                    if layout.get(c).is_some_and(|ch| i < ch.leds()) {
                        paint(c, i);
                    }
                }
            }
        }
    }

    let master = store.brightness as f32 / 100.0;
    for c in frame.iter_mut().flatten() {
        *c = c.scale(master);
    }
    frame
}

fn layer_color(l: &Layer, s: &Spot, fans: usize, t: f64) -> Rgb {
    let fans = fans.max(1);
    let around = s.k as f64 / s.ring_len.max(1) as f64; // 0..1 around its ring
    let x = match l.spread {
        Spread::Across => (s.fan as f64 + around) / fans as f64,
        Spread::PerFan => around,
    };
    let speed = l.speed as f64;

    match l.effect {
        Effect::Static => l.color1,
        Effect::Gradient => {
            let p = match l.spread {
                // reach color 2 exactly on the last fan's last LED
                Spread::Across => {
                    let steps = (fans * s.ring_len) as f64 - 1.0;
                    if steps > 0.0 { (s.fan * s.ring_len + s.k) as f64 / steps } else { 0.0 }
                }
                Spread::PerFan => there_and_back(around),
            };
            l.color1.mix(l.color2, p as f32)
        }
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
            // The head goes once round each ring per lap; across fans it
            // travels fan to fan.
            let laps = match l.spread {
                Spread::Across => fans as f64,
                Spread::PerFan => 1.0,
            };
            let head = (t * speed * 0.12 / laps).rem_euclid(1.0);
            let behind = (head - x).rem_euclid(1.0) * laps; // 0 at the head, in ring-lengths
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
        LightingStore { active: 0, brightness: 100, profiles: vec![Profile { name: "T".into(), layers: vec![layer] }] }
    }

    fn fx(effect: Effect, spread: Spread) -> Layer {
        Layer { effect, spread, ..Default::default() }
    }

    #[test]
    fn frame_matches_layout() {
        let f = render(&LightingStore::starter(&three_fans()), &three_fans(), 0.0);
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
            profiles: vec![Profile { name: "Hub & Ring".into(), layers: vec![rings, hubs] }],
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
            profiles: vec![Profile { name: "x".repeat(100), layers: vec![layer; MAX_LAYERS + 5] }],
        }
        .sanitized(&layout);
        assert_eq!((s.active, s.brightness), (0, 100));
        assert_eq!(s.profiles[0].name.chars().count(), MAX_NAME_LEN);
        assert_eq!(s.profiles[0].layers.len(), MAX_LAYERS);
        let l = &s.profiles[0].layers[0];
        assert_eq!((l.brightness, l.speed, l.name.as_str()), (100, 1, "Layer"));
        assert_eq!(l.target, Target::Leds { leds: vec![LedRef(0, 47)] });

        let empty = LightingStore { active: 0, brightness: 100, profiles: vec![] }.sanitized(&layout);
        assert!(!empty.profiles.is_empty());
    }

    #[test]
    fn store_json_round_trip() {
        let layout = three_fans();
        let s = LightingStore::starter(&layout);
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""kind":"all""#));
        assert_eq!(serde_json::from_str::<LightingStore>(&json).unwrap(), s);
        assert!(serde_json::from_str::<LightingStore>(r#"{"effect":"wave"}"#).is_err());
    }
}
