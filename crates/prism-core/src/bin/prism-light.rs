//! `prism-light`: test tool that sets the Lighting Node Pro's colors.
//!
//!   prism-light ff2d95        both channels hot pink
//!   prism-light ember         purple-to-pink gradient
//!   prism-light ember 30      ...and hold it for 30 seconds (default 15)
//!
//! When time is up the device goes back to its own saved effect.

use std::time::{Duration, Instant};

use hidapi::HidApi;
use prism_core::lighting_node::{LightingNode, CHANNELS, KEEPALIVE_INTERVAL, MAX_LEDS_PER_CHANNEL};
use prism_core::Rgb;

const PURPLE: Rgb = Rgb::new(0x7c, 0x3a, 0xed);
const HOT_PINK: Rgb = Rgb::new(0xff, 0x2d, 0x95);

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let Some(what) = args.next() else {
        eprintln!("Usage: prism-light <hex color | ember> [seconds]");
        std::process::exit(2);
    };
    let seconds: u64 = match args.next() {
        Some(s) => s.parse().map_err(|_| format!("'{s}' is not a number of seconds"))?,
        None => 15,
    };

    let colors: Vec<Rgb> = if what == "ember" {
        // Spread the gradient over the first 48 LEDs, then repeat it.
        (0..MAX_LEDS_PER_CHANNEL).map(|i| PURPLE.mix(HOT_PINK, (i % 48) as f32 / 47.0)).collect()
    } else {
        let c = Rgb::from_hex(&what).ok_or(format!("'{what}' is not a color like ff2d95"))?;
        vec![c; MAX_LEDS_PER_CHANNEL]
    };

    let api = HidApi::new()?;
    let node = LightingNode::open(&api)?;
    let (a, b, c) = node.firmware_version()?;
    println!("Lighting Node Pro found, firmware {a}.{b}.{c}");

    for ch in 0..CHANNELS {
        node.set_colors(ch, &colors)?;
    }
    println!("Colors set. Holding for {seconds} s...");

    let end = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < end {
        std::thread::sleep(KEEPALIVE_INTERVAL.min(end.saturating_duration_since(Instant::now())));
        node.keepalive()?;
    }

    node.release()?;
    println!("Done. Handed control back to the device.");
    Ok(())
}
