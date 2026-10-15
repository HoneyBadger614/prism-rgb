//! Lighting effects. [`render`] works out every LED's color for a moment in
//! time; the app calls it many times a second and sends the result to the
//! device. It's pure math, so the preview and the device always agree.

use serde::{Deserialize, Serialize};

use crate::Rgb;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    /// One color everywhere.
    Static,
    /// Color 1 fading into color 2.
    Gradient,
    /// The gradient flowing along the LEDs.
    Wave,
    /// Fading in and out, alternating between the two colors.
    Breathe,
}

/// How a gradient or wave is laid out over the fans.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Spread {
    /// One long fade from the first fan to the last.
    Across,
    /// Each fan gets the full fade around its own ring.
    PerFan,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LightingConfig {
    pub effect: Effect,
    pub color1: Rgb,
    pub color2: Rgb,
    pub spread: Spread,
    /// 0–100 percent.
    pub brightness: u8,
    /// 1 (slow) to 10 (fast).
    pub speed: u8,
}

impl Default for LightingConfig {
    fn default() -> Self {
        Self {
            effect: Effect::Gradient,
            color1: Rgb::new(0x7c, 0x3a, 0xed),
            color2: Rgb::new(0xff, 0x2d, 0x95),
            spread: Spread::Across,
            brightness: 100,
            speed: 5,
        }
    }
}

impl LightingConfig {
    /// Pulls out-of-range numbers back into range.
    pub fn sanitized(mut self) -> Self {
        self.brightness = self.brightness.min(100);
        self.speed = self.speed.clamp(1, 10);
        self
    }

    /// Whether the colors change over time (and so need constant updates).
    pub fn is_animated(&self) -> bool {
        matches!(self.effect, Effect::Wave | Effect::Breathe)
    }
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

/// Where one LED sits: which fan (counting across all channels), and its
/// place on its own ring.
struct Spot {
    fan: usize,
    k: usize,
    ring_len: usize,
}

/// 0 → 0, 0.5 → 1, 1 → 0. Makes a fade that loops without a visible seam.
fn there_and_back(u: f64) -> f64 {
    1.0 - (2.0 * u - 1.0).abs()
}

/// Every LED's color at `t` seconds, one list per channel.
pub fn render(cfg: &LightingConfig, layout: &[ChannelLayout], t: f64) -> Vec<Vec<Rgb>> {
    let total_fans: usize = layout.iter().map(|c| c.fans).sum();
    let level = cfg.brightness as f32 / 100.0;
    let mut first_fan = 0;

    layout
        .iter()
        .map(|ch| {
            let per = ch.leds_per_fan().max(1);
            let colors = (0..ch.leds())
                .map(|i| {
                    let j = i % per;
                    let (k, ring_len) = if j < ch.inner { (j, ch.inner) } else { (j - ch.inner, ch.outer) };
                    let spot = Spot { fan: first_fan + i / per, k, ring_len };
                    color_at(cfg, &spot, total_fans, t).scale(level)
                })
                .collect();
            first_fan += ch.fans;
            colors
        })
        .collect()
}

fn color_at(cfg: &LightingConfig, s: &Spot, fans: usize, t: f64) -> Rgb {
    let around = s.k as f64 / s.ring_len.max(1) as f64; // 0..1 around its ring
    let x = match cfg.spread {
        Spread::Across => (s.fan as f64 + around) / fans.max(1) as f64,
        Spread::PerFan => around,
    };
    let speed = cfg.speed as f64;

    match cfg.effect {
        Effect::Static => cfg.color1,
        Effect::Gradient => {
            let p = match cfg.spread {
                // reach color 2 exactly on the last fan's last LED
                Spread::Across => {
                    let steps = (fans.max(1) * s.ring_len) as f64 - 1.0;
                    if steps > 0.0 { (s.fan * s.ring_len + s.k) as f64 / steps } else { 0.0 }
                }
                Spread::PerFan => there_and_back(around),
            };
            cfg.color1.mix(cfg.color2, p as f32)
        }
        Effect::Wave => {
            let phase = (x - t * speed * 0.06).rem_euclid(1.0);
            cfg.color1.mix(cfg.color2, there_and_back(phase) as f32)
        }
        Effect::Breathe => {
            let phase = t * speed * 0.05;
            let c = if (phase.floor() as i64) % 2 == 0 { cfg.color1 } else { cfg.color2 };
            let s = (std::f64::consts::PI * phase.fract()).sin();
            c.scale((s * s) as f32)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PURPLE: Rgb = Rgb::new(0x7c, 0x3a, 0xed);
    const PINK: Rgb = Rgb::new(0xff, 0x2d, 0x95);

    fn three_fans() -> Vec<ChannelLayout> {
        vec![ChannelLayout::ll_fans(3), ChannelLayout::ll_fans(0)]
    }

    fn cfg(effect: Effect, spread: Spread) -> LightingConfig {
        LightingConfig { effect, spread, ..Default::default() }
    }

    #[test]
    fn frame_matches_layout() {
        let f = render(&LightingConfig::default(), &three_fans(), 0.0);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].len(), 48);
        assert!(f[1].is_empty());
    }

    #[test]
    fn static_is_one_color() {
        let f = render(&cfg(Effect::Static, Spread::Across), &three_fans(), 3.0);
        assert!(f[0].iter().all(|&c| c == PURPLE));
    }

    #[test]
    fn gradient_across_runs_first_to_last() {
        let f = render(&cfg(Effect::Gradient, Spread::Across), &three_fans(), 0.0);
        assert_eq!(f[0][0], PURPLE); // fan 1, first hub LED
        assert_eq!(f[0][4], PURPLE); // fan 1, first ring LED
        assert_eq!(f[0][35], PINK); // fan 3, last hub LED
        assert_eq!(f[0][47], PINK); // fan 3, last ring LED
    }

    #[test]
    fn hub_and_ring_line_up_across_fans() {
        // Fan 2's hub should sit in the middle of the fade, like its ring.
        let f = render(&cfg(Effect::Gradient, Spread::Across), &three_fans(), 0.0);
        let hub = f[0][16 + 2];
        let ring = f[0][20 + 6];
        assert!((hub.r as i32 - ring.r as i32).abs() <= 12, "{hub:?} vs {ring:?}");
    }

    #[test]
    fn gradient_per_fan_repeats_on_each_fan() {
        let f = render(&cfg(Effect::Gradient, Spread::PerFan), &three_fans(), 0.0);
        for fan in 0..3 {
            assert_eq!(f[0][fan * 16], PURPLE); // hub start
            assert_eq!(f[0][fan * 16 + 2], PINK); // halfway round the hub
            assert_eq!(f[0][fan * 16 + 4], PURPLE); // ring start
            assert_eq!(f[0][fan * 16 + 10], PINK); // halfway round the ring
            assert_eq!(f[0][fan * 16..fan * 16 + 16], f[0][0..16]);
        }
    }

    #[test]
    fn wave_moves() {
        let c = cfg(Effect::Wave, Spread::Across);
        assert_ne!(render(&c, &three_fans(), 0.0), render(&c, &three_fans(), 1.0));
    }

    #[test]
    fn breathe_goes_dark_between_breaths() {
        let c = cfg(Effect::Breathe, Spread::Across);
        // speed 5 -> one breath every 4 s; t = 0 is the dark point
        assert!(render(&c, &three_fans(), 0.0)[0].iter().all(|&x| x == Rgb::new(0, 0, 0)));
        assert_eq!(render(&c, &three_fans(), 2.0)[0][0], PURPLE); // peak of breath 1
        assert_eq!(render(&c, &three_fans(), 6.0)[0][0], PINK); // peak of breath 2
    }

    #[test]
    fn brightness_zero_is_off() {
        let c = LightingConfig { brightness: 0, ..Default::default() };
        assert!(render(&c, &three_fans(), 0.0)[0].iter().all(|&x| x == Rgb::new(0, 0, 0)));
    }

    #[test]
    fn sanitize_clamps() {
        let c = LightingConfig { brightness: 250, speed: 0, ..Default::default() }.sanitized();
        assert_eq!((c.brightness, c.speed), (100, 1));
    }

    #[test]
    fn config_reads_from_json() {
        let c: LightingConfig =
            serde_json::from_str(r##"{"effect":"wave","color1":"#ff0000","spread":"per-fan"}"##).unwrap();
        assert_eq!(c.effect, Effect::Wave);
        assert_eq!(c.color1, Rgb::new(255, 0, 0));
        assert_eq!(c.spread, Spread::PerFan);
        assert_eq!(c.color2, PINK); // missing fields use defaults
        assert!(serde_json::from_str::<LightingConfig>(r#"{"color1":"red"}"#).is_err());
    }
}
