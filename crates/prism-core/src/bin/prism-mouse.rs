//! `prism-mouse`: test tool for the Nightsword RGB's lighting.
//!
//!   prism-mouse ff2d95       every zone hot pink, for 15 s
//!   prism-mouse ff2d95 30    ...for 30 s
//!   prism-mouse zones        light each zone white in turn, to check them
//!   prism-mouse zone 4 20    light only zone 4 (of 5) white, for 20 s
//!   prism-mouse probe 5 8 4  try raw zone ids 5..=8, each white for 4 s
//!   prism-mouse buttons Sniper 15
//!                            for 15 s, the Sniper button reports to the
//!                            computer; presses are shown, then it's put back
//!   prism-mouse dpi 400 1600 3200 800
//!                            set stages 1-3 and sniper, then try each for 8 s
//!   prism-mouse backup DIR   copy every onboard file into DIR (read-only)
//!   prism-mouse reset        just hand lighting back to the mouse
//!
//! When time is up the mouse goes back to its own lighting.

use std::time::{Duration, Instant};

use hidapi::HidApi;
use prism_core::nightsword::{Nightsword, KEEPALIVE_INTERVAL, ZONES, ZONE_COUNT};
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
    if what == "backup" {
        let dir = std::path::PathBuf::from(args.next().ok_or("give a folder to save into")?);
        std::fs::create_dir_all(&dir)?;
        // Opened without switching modes: reading only.
        let mouse = Nightsword::open_with(&api, false)?;
        for (folder, name, data) in mouse.read_all_files(&api)? {
            std::fs::write(dir.join(format!("{folder}_{name}")), &data)?;
            println!("  folder {folder} {name}: {} bytes", data.len());
        }
        return Ok(());
    }
    let mouse = Nightsword::open(&api)?;
    println!("Nightsword RGB found and switched to Prism control.");

    if what == "dpi" {
        let v: Vec<u16> = args.filter_map(|s| s.parse().ok()).collect();
        let [a, b, c, sniper] = v[..] else {
            return Err("give four numbers: stage 1, stage 2, stage 3, sniper".into());
        };
        mouse.set_dpi_stages([a, b, c], sniper)?;
        for (stage, label) in [(1u8, a), (2, b), (3, c), (0, sniper)] {
            let name = if stage == 0 { "sniper".to_string() } else { format!("stage {stage}") };
            println!("  {name}: {label} DPI for 8 s - move the mouse");
            mouse.select_dpi(stage)?;
            std::thread::sleep(Duration::from_secs(8));
        }
        println!("Handing DPI back to the mouse.");
    } else if what == "buttons" {
        let name = args.next().unwrap_or_else(|| "Sniper".into());
        let seconds: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(15);
        let channel = Nightsword::open_buttons(&api)?;
        mouse.report_buttons(&[name.as_str()])?;
        println!("{name} now reports to the computer for {seconds} s. Press it a few times.");
        let end = Instant::now() + Duration::from_secs(seconds);
        let mut last: Vec<&str> = Vec::new();
        let mut buf = [0u8; 64];
        while Instant::now() < end {
            let n = channel.read_timeout(&mut buf, 100)?;
            if n == 0 {
                continue;
            }
            if let Some(held) = prism_core::nightsword::buttons_held(&buf[..n]) {
                if held != last {
                    println!("  held: {}", if held.is_empty() { "(none)".to_string() } else { held.join(", ") });
                    last = held;
                }
            }
        }
        println!("Putting every button back to normal.");
    } else if what == "probe" {
        let from: u8 = args.next().and_then(|s| s.parse().ok()).unwrap_or(5);
        let to: u8 = args.next().and_then(|s| s.parse().ok()).unwrap_or(8);
        let hold: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(4);
        let black = Rgb::new(0, 0, 0);
        for id in from..=to {
            println!("  zone id {id}: white for {hold} s");
            // known zones off, then just this id white
            let mut zones: Vec<(u8, Rgb)> = ZONES.iter().map(|(_, z)| (*z, black)).collect();
            zones.push((id, Rgb::new(255, 255, 255)));
            for _ in 0..hold {
                mouse.set_zone_ids(&zones)?;
                std::thread::sleep(Duration::from_secs(1));
            }
            mouse.set_zone_ids(&[(id, black)])?;
            std::thread::sleep(Duration::from_millis(300));
        }
    } else if what == "zone" {
        let n: usize = args.next().and_then(|s| s.parse().ok()).filter(|n| (1..=ZONE_COUNT).contains(n)).ok_or("give a zone number, 1-5")?;
        let seconds: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(15);
        let mut colors = [Rgb::new(0, 0, 0); ZONE_COUNT];
        colors[n - 1] = Rgb::new(255, 255, 255);
        println!("Zone {n} ({}) white for {seconds} s.", ZONES[n - 1].0);
        let end = Instant::now() + Duration::from_secs(seconds);
        while Instant::now() < end {
            mouse.set_colors(&colors)?;
            std::thread::sleep(Duration::from_secs(1));
        }
    } else if what == "reset" {
        // nothing to show; fall through to handing back
    } else if what == "zones" {
        for (i, (name, _)) in ZONES.iter().enumerate() {
            println!("  zone {}: {name}", i + 1);
            let mut colors = [Rgb::new(0, 0, 0); ZONE_COUNT];
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
        let colors = [c; ZONE_COUNT];
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
