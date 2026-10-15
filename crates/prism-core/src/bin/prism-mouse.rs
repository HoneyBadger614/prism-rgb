//! `prism-mouse`: test tool for the Nightsword RGB's lighting.
//!
//!   prism-mouse ff2d95       every zone hot pink, for 15 s
//!   prism-mouse ff2d95 30    ...for 30 s
//!   prism-mouse zones        light each zone white in turn, to check them
//!   prism-mouse reset        just hand lighting back to the mouse
//!
//! When time is up the mouse goes back to its own lighting.

use std::time::{Duration, Instant};

use hidapi::HidApi;
use prism_core::nightsword::{Nightsword, KEEPALIVE_INTERVAL, ZONES};
use prism_core::Rgb;

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let Some(what) = args.next() else {
        eprintln!("Usage: prism-mouse <hex color | zones | reset> [seconds]");
        std::process::exit(2);
    };

    let api = HidApi::new()?;
    let mouse = Nightsword::open(&api)?;
    println!("Nightsword RGB found and switched to Prism control.");

    if what == "reset" {
        // nothing to show; fall through to handing back
    } else if what == "zones" {
        for (i, (name, _)) in ZONES.iter().enumerate() {
            println!("  zone {}: {name}", i + 1);
            let mut colors = [Rgb::new(0, 0, 0); 4];
            colors[i] = Rgb::new(255, 255, 255);
            // hold each for 3 s, refreshing so the mouse stays in software mode
            for _ in 0..3 {
                mouse.set_colors(&colors)?;
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    } else {
        let seconds: u64 = match args.next() {
            Some(s) => s.parse().map_err(|_| format!("'{s}' is not a number of seconds"))?,
            None => 15,
        };
        let c = Rgb::from_hex(&what).ok_or(format!("'{what}' is not a color like ff2d95"))?;
        let colors = [c; 4];
        mouse.set_colors(&colors)?;
        println!("Colors set. Holding for {seconds} s...");
        let end = Instant::now() + Duration::from_secs(seconds);
        while Instant::now() < end {
            std::thread::sleep(KEEPALIVE_INTERVAL.min(end.saturating_duration_since(Instant::now())));
            mouse.set_colors(&colors)?;
        }
    }

    mouse.release()?;
    println!("Done. Handed lighting back to the mouse.");
    Ok(())
}
