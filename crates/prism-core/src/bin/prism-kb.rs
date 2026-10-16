//! `prism-kb`: test tool for the K65 RGB Mini's lighting.
//!
//!   prism-kb ff2d95        every key hot pink, for 15 s
//!   prism-kb ff2d95 30     ...for 30 s
//!   prism-kb slots         light slots one at a time and print each number,
//!                          to check which slot is which key
//!   prism-kb backup DIR    copy every onboard file into DIR (read-only)
//!   prism-kb rewrite 6d6a  write one onboard file back with its OWN contents,
//!                          then read it again and compare (tests writing)
//!   prism-kb write 6d6a FILE   replace onboard file 6d6a with FILE's bytes
//!
//! When time is up the keyboard goes back to its own lighting. Typing keeps
//! working the whole time.

use std::time::{Duration, Instant};

use hidapi::HidApi;
use prism_core::k65_mini::{K65Mini, KEEPALIVE_INTERVAL, SLOTS};
use prism_core::Rgb;

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
        eprintln!("Usage: prism-kb <hex color | slots> [seconds]");
        std::process::exit(2);
    };

    let api = HidApi::new()?;
    if what == "backup" {
        let dir = std::path::PathBuf::from(args.next().ok_or("give a folder to save into")?);
        std::fs::create_dir_all(&dir)?;
        // The keyboard only opens its files to the program in control, so
        // take control for the read and hand it back afterwards.
        let kb = K65Mini::open_with(&api, true)?;
        for (id, r) in kb.read_all_files() {
            match r {
                Ok(data) => {
                    std::fs::write(dir.join(format!("{id:04x}.bin")), &data)?;
                    println!("  file {id:#06x}: {} bytes", data.len());
                }
                Err(e) => println!("  file {id:#06x}: {e}"),
            }
        }
        kb.release()?;
        return Ok(());
    }
    if what == "write" {
        let arg = args.next().ok_or("give a file number, e.g. 6d6a")?;
        let id = u16::from_str_radix(arg.trim_start_matches("0x"), 16).map_err(|_| format!("'{arg}' isn't a file number"))?;
        let data = std::fs::read(args.next().ok_or("give a file to write")?)?;
        let kb = K65Mini::open_with(&api, true)?; // files only open to the program in control
        kb.write_file(id, &data)?;
        let after = kb.read_file(id)?;
        println!("wrote file {id:#06x}: now {}", hex(&after));
        kb.release()?;
        return Ok(());
    }
    if what == "rewrite" {
        let arg = args.next().ok_or("give a file number, e.g. 6d6a")?;
        let id = u16::from_str_radix(arg.trim_start_matches("0x"), 16).map_err(|_| format!("'{arg}' isn't a file number"))?;
        let kb = K65Mini::open_with(&api, true)?; // files only open to the program in control
        let before = kb.read_file(id)?;
        println!("read file {id:#06x}: {} bytes: {}", before.len(), hex(&before));
        kb.write_file(id, &before)?;
        let after = kb.read_file(id)?;
        println!("wrote it back; read again: {} bytes", after.len());
        println!("{}", if after == before { "IDENTICAL: writing works" } else { "DIFFERENT!" });
        kb.release()?;
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

fn hex(b: &[u8]) -> String {
    b.iter().take(32).map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")
}
