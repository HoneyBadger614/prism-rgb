//! `prism-kb`: test tool for the K65 RGB Mini's lighting.
//!
//!   prism-kb ff2d95        every key hot pink, for 15 s
//!   prism-kb ff2d95 30     ...for 30 s
//!   prism-kb slots         light slots one at a time and print each number,
//!                          to check which slot is which key
//!   prism-kb backup DIR    copy every onboard file into DIR (read-only)
//!
//! When time is up the keyboard goes back to its own lighting. Typing keeps
//! working the whole time.

use std::time::{Duration, Instant};

use hidapi::HidApi;
use prism_core::k65_mini::{K65Mini, KEEPALIVE_INTERVAL, SLOTS};
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
        eprintln!("Usage: prism-kb <hex color | slots> [seconds]");
        std::process::exit(2);
    };

    let api = HidApi::new()?;
    if what == "backup" {
        let dir = std::path::PathBuf::from(args.next().ok_or("give a folder to save into")?);
        std::fs::create_dir_all(&dir)?;
        // Opened without taking over the lighting: reading only.
        let kb = K65Mini::open_with(&api, false)?;
        for (id, r) in kb.read_all_files() {
            match r {
                Ok(data) => {
                    std::fs::write(dir.join(format!("{id:04x}.bin")), &data)?;
                    println!("  file {id:#06x}: {} bytes", data.len());
                }
                Err(e) => println!("  file {id:#06x}: {e}"),
            }
        }
        return Ok(());
    }
    let kb = K65Mini::open(&api)?;
    println!("K65 RGB Mini found and switched to Prism control.");

    if what == "slots" {
        // A few known keys, so the person can say whether they lit up right.
        for (slot, name) in [(41u8, "Esc"), (4, "A"), (44, "Space (middle)"), (0, "Space (left)"), (1, "Space (right)"), (42, "Backspace")] {
            println!("  slot {slot:>3}: should be {name}");
            kb.set_slots(&[(slot, Rgb::new(255, 255, 255))])?;
            std::thread::sleep(Duration::from_secs(2));
        }
    } else {
        let seconds: u64 = match args.next() {
            Some(s) => s.parse().map_err(|_| format!("'{s}' is not a number of seconds"))?,
            None => 15,
        };
        let c = Rgb::from_hex(&what).ok_or(format!("'{what}' is not a color like ff2d95"))?;
        let colors = [c; SLOTS];
        kb.set_colors(&colors)?;
        println!("Colors set. Holding for {seconds} s...");
        let end = Instant::now() + Duration::from_secs(seconds);
        while Instant::now() < end {
            std::thread::sleep(KEEPALIVE_INTERVAL.min(end.saturating_duration_since(Instant::now())));
            kb.set_colors(&colors)?;
        }
    }

    kb.release()?;
    println!("Done. Handed lighting back to the keyboard.");
    Ok(())
}
