//! `prism-light`: test tool that sets the Lighting Node Pro's colors.
//!
//!   prism-light ff2d95        both channels hot pink
//!   prism-light ember         purple-to-pink gradient
//!   prism-light ember 30      ...and hold it for 30 seconds (default 15)
//!   prism-light fan-order     white hubs, red rings: checks the LED order
//!   prism-light identify 2    channel 2 only: a white dot runs along it
//!                             (channel 1 goes dark), to see what's plugged in
//!
//! When time is up the device goes back to its own saved effect.

use std::time::{Duration, Instant};

use hidapi::HidApi;
use prism_core::lighting_node::{LightingNode, CHANNELS, KEEPALIVE_INTERVAL, MAX_LEDS_PER_CHANNEL};
use prism_core::Rgb;

const PURPLE: Rgb = Rgb::new(0x7c, 0x3a, 0xed);
const HOT_PINK: Rgb = Rgb::new(0xff, 0x2d, 0x95);

fn main() {
    if prism_core::prism_app_running() {
        eprintln!("Prism is running. Quit it first (tray icon -> Quit Prism): two programs talking to one device get mixed up.");
        std::process::exit(1);
    }
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
    if what == "fan-order" {
        return fan_order();
    }

    if what == "identify" {
        let channel: u8 = match args.next().as_deref() {
            Some("1") => 1,
            Some("2") | None => 2,
            Some(s) => return Err(format!("'{s}' is not a channel (use 1 or 2)").into()),
        };
        return identify(channel);
    }

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
        node.set_brightness(ch, 100)?;
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

/// All fans, for 30 s: LEDs 1-4 of each fan white, LEDs 5-16 red. Prism
/// expects LEDs 1-4 to be the center hub, so the hub should be white.
fn fan_order() -> Result<(), Box<dyn std::error::Error>> {
    let api = HidApi::new()?;
    let node = LightingNode::open(&api)?;
    let colors: Vec<Rgb> = (0..48)
        .map(|i| if i % 16 < 4 { Rgb::new(255, 255, 255) } else { Rgb::new(255, 0, 0) })
        .collect();
    node.set_brightness(0, 100)?;
    node.set_colors(0, &colors)?;
    println!("Every fan: LEDs 1-4 white, LEDs 5-16 red. Expected: white center, red outer ring. 30 s.");
    let end = Instant::now() + Duration::from_secs(30);
    while Instant::now() < end {
        std::thread::sleep(KEEPALIVE_INTERVAL.min(end.saturating_duration_since(Instant::now())));
        node.keepalive()?;
    }
    node.release()?;
    println!("Done. Handed control back to the device.");
    Ok(())
}

/// Lights one channel with a running white dot and green trail for 30 s,
/// with the other channel dark. Prints which LED is lit once per lap.
fn identify(channel: u8) -> Result<(), Box<dyn std::error::Error>> {
    const LEDS: usize = 16; // what OpenRGB reported for channel 2
    const WHITE: Rgb = Rgb::new(255, 255, 255);
    const GREEN: Rgb = Rgb::new(0, 255, 0);
    let index = channel - 1;

    let api = HidApi::new()?;
    let node = LightingNode::open(&api)?;
    for ch in 0..CHANNELS {
        node.set_brightness(ch, 100)?;
    }
    node.set_colors(1 - index, &vec![Rgb::new(0, 0, 0); MAX_LEDS_PER_CHANNEL])?;
    println!("Channel {channel}: white dot with a green trail for 30 s. The other channel is dark.");

    let end = Instant::now() + Duration::from_secs(30);
    let mut step = 0usize;
    while Instant::now() < end {
        let pos = step % LEDS;
        let frame: Vec<Rgb> = (0..LEDS)
            .map(|i| match (pos + LEDS - i) % LEDS {
                0 => WHITE,
                d @ 1..=3 => Rgb::new(0, 0, 0).mix(GREEN, 1.0 - d as f32 / 4.0),
                _ => Rgb::new(0, 0, 0),
            })
            .collect();
        node.set_colors(index, &frame)?;
        // the dark channel needs a keepalive too; set_colors commits both
        std::thread::sleep(Duration::from_millis(120));
        step += 1;
    }

    node.release()?;
    println!("Done. Handed control back to the device.");
    Ok(())
}
