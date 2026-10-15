//! `prism-input`: test tool for Prism's input layer.
//!
//!   prism-input list         show the Corsair input devices Prism can see
//!   prism-input watch 30     for 30 s, show which key or button codes the
//!                            Corsair devices send (shown on screen only)
//!   prism-input remap kb KEY_Q KEY_Z 30
//!                            for 30 s, Q on the keyboard types Z instead
//!                            ("kb" or "mouse"; everything else works as normal)
//!
//! `watch` does not take anything over: keys and buttons keep working as
//! normal while it runs. Don't type passwords while it's running.

use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};

use evdev::{InputEventKind, Key};
use prism_input::{corsair_devices, key_code, Action, Binding, Remapper};

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("list") => list(),
        Some("watch") => watch(args.next().and_then(|s| s.parse().ok()).unwrap_or(20)),
        Some("capture") => {
            let product = match args.next().as_deref() {
                Some("kb") => prism_core_pids::KEYBOARD,
                _ => prism_core_pids::MOUSE,
            };
            println!("Press a button within 10 s...");
            match prism_input::capture_press(product, Duration::from_secs(10)) {
                Some(c) => println!("Got {} ({c})", prism_input::raw_name(c)),
                None => println!("Nothing pressed."),
            }
        }
        Some("remap") => {
            let rest: Vec<String> = args.collect();
            if let Err(e) = remap(&rest) {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
        _ => {
            eprintln!("Usage: prism-input <list | watch [seconds] | remap kb|mouse FROM TO [seconds]>");
            std::process::exit(2);
        }
    }
}

fn list() {
    for d in corsair_devices() {
        let keys = d.device.supported_keys().map_or(0, |k| k.iter().count());
        println!("{}  [{:04x}]  {}  ({keys} keys/buttons)", d.path.display(), d.product, d.name);
    }
}

fn watch(seconds: u64) {
    let mut devices: Vec<_> = corsair_devices().into_iter().filter(|d| d.has_keys()).collect();
    if devices.is_empty() {
        eprintln!("No Corsair keyboards or mice found.");
        std::process::exit(1);
    }
    for d in &mut devices {
        // Non-blocking, so one quiet device doesn't hold up the others.
        let fd = d.device.as_raw_fd();
        unsafe {
            let flags = libc_fcntl(fd, F_GETFL, 0);
            libc_fcntl(fd, F_SETFL, flags | O_NONBLOCK);
        }
    }
    println!("Watching {} Corsair input devices for {seconds} s. Press the buttons you want to check.", devices.len());

    let end = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < end {
        for d in &mut devices {
            let Ok(events) = d.device.fetch_events() else { continue };
            for ev in events {
                if let InputEventKind::Key(key) = ev.kind() {
                    let state = match ev.value() {
                        1 => "pressed",
                        0 => "released",
                        _ => continue, // key repeat
                    };
                    println!("  {:<28} {:<20} {state}", short_name(&d.name), name(key));
                }
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    println!("Done.");
}

fn remap(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let [which, from, to, rest @ ..] = args else {
        return Err("give: kb|mouse FROM TO [seconds], e.g. kb KEY_Q KEY_Z 30".into());
    };
    let product = match which.as_str() {
        "kb" => prism_core_pids::KEYBOARD,
        "mouse" => prism_core_pids::MOUSE,
        _ => return Err(format!("'{which}' should be kb or mouse").into()),
    };
    let input = key_code(from).ok_or(format!("unknown key '{from}'"))?;
    let output = key_code(to).ok_or(format!("unknown key '{to}'"))?;
    let seconds: u64 = rest.first().and_then(|s| s.parse().ok()).unwrap_or(20);

    let r = Remapper::start(&[Binding { product, input, action: Action::Key(output) }], None)?;
    if r.grabbed.is_empty() {
        return Err(format!("no Corsair {which} sends {from}").into());
    }
    println!("For {seconds} s, {from} now acts as {to}. Everything else works as normal.");
    std::thread::sleep(Duration::from_secs(seconds));
    r.stop();
    println!("Done. Devices handed back.");
    Ok(())
}

mod prism_core_pids {
    pub const KEYBOARD: u16 = 0x1baf;
    pub const MOUSE: u16 = 0x1b5c;
}

fn name(key: Key) -> String {
    format!("{key:?}")
}

fn short_name(name: &str) -> String {
    name.replace("Corsair CORSAIR ", "").replace(" Gaming Mouse", " mouse").replace(" 60% Mechanical Gaming Keyboard", " keyboard")
}

// Minimal fcntl bindings so we don't need another dependency just for this.
const F_GETFL: i32 = 3;
const F_SETFL: i32 = 4;
const O_NONBLOCK: i32 = 0o4000;
extern "C" {
    #[link_name = "fcntl"]
    fn libc_fcntl(fd: i32, cmd: i32, ...) -> i32;
}
