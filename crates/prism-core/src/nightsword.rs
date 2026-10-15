//! Corsair Nightsword RGB (1b1c:1b5c).
//!
//! It uses Corsair's older mouse protocol: 64-byte HID reports (plus a
//! leading report number of 0) sent to USB interface 1. Writes start with
//! 0x07.
//!
//! To show our own colors we switch the mouse to software mode, then send
//! one message with a color for each of its four lighting zones. Colors are
//! re-sent every few seconds so the mouse stays in software mode.

use std::ffi::CString;
use std::time::Duration;

use hidapi::{HidApi, HidDevice};

use crate::{find_devices, Rgb};

pub const PID: u16 = 0x1b5c;
/// Write colors at least this often.
pub const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(4);

const INTERFACE: i32 = 1;
const REPORT_LEN: usize = 65; // report number + 64 bytes
const WRITE: u8 = 0x07;
/// Pause between back-to-back messages.
const GAP: Duration = Duration::from_millis(50);

mod cmd {
    pub const MODE: u8 = 0x04;
    pub const COLOR: u8 = 0x22;
    pub const BUTTON_SETUP: u8 = 0x40;
}

/// Per-button setting: the mouse handles the button itself (normal), or
/// reports presses to the computer on its button channel.
mod button_mode {
    pub const MOUSE: u8 = 0xc0;
    pub const REPORT: u8 = 0x40;
}

/// Each button's number in the button-setup message and its bit in button
/// reports, in the order the mouse expects them.
const BUTTON_IDS: [(&str, u8, u16); 10] = [
    ("Left", 1, 1 << 0),
    ("Right", 2, 1 << 1),
    ("Middle", 3, 1 << 2),
    ("Back", 4, 1 << 3),
    ("Forward", 5, 1 << 4),
    ("DPI Up", 6, 1 << 5),
    ("DPI Down", 7, 1 << 6),
    ("Sniper", 8, 1 << 7),
    ("Profile Up", 25, 1 << 8),
    ("Profile Down", 26, 1 << 9),
];

/// Button reports arrive on interface 0 as HID report 3.
const BUTTON_INTERFACE: i32 = 0;
const BUTTON_REPORT_ID: u8 = 3;

mod mode {
    /// The mouse runs its own saved lighting.
    pub const HARDWARE: u8 = 0x01;
    /// The computer sends colors directly.
    pub const SOFTWARE: u8 = 0x02;
}

/// The mouse's lighting zones, in the order Prism uses them, with the id
/// each has in the color message. Checked on the real mouse: id 3 is the
/// DPI indicator and id 6 the back light under the logo.
pub const ZONES: [(&str, u8); 5] =
    [("Front", 1), ("Scroll wheel", 4), ("Logo", 2), ("DPI indicator", 3), ("Back", 6)];
pub const ZONE_COUNT: usize = ZONES.len();

/// The mouse's 10 buttons and the code bindings use for each. The first
/// five send standard Linux button codes. The rest are handled inside the
/// mouse and send nothing to Linux, so Prism gives them codes of its own
/// (0x2f0 up, which Linux doesn't use) and, when one is bound, has the mouse
/// report it on its button channel instead.
pub const BUTTONS: [(&str, u16); 10] = [
    ("Left", 0x110),
    ("Right", 0x111),
    ("Middle", 0x112),
    ("Back", 0x113),
    ("Forward", 0x114),
    ("DPI Up", 0x2f0),
    ("DPI Down", 0x2f1),
    ("Sniper", 0x2f2),
    ("Profile Up", 0x2f3),
    ("Profile Down", 0x2f4),
];

/// The button behind one of Prism's own codes (the ones the mouse only
/// reports on its button channel).
pub fn reported_button(code: u16) -> Option<&'static str> {
    BUTTONS.iter().skip(5).find(|(_, c)| *c == code).map(|(n, _)| *n)
}

#[derive(Debug)]
pub enum Error {
    NotFound,
    Usb(hidapi::HidError),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotFound => write!(f, "Nightsword RGB is not connected"),
            Error::Usb(e) => write!(f, "USB error: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<hidapi::HidError> for Error {
    fn from(e: hidapi::HidError) -> Self {
        Error::Usb(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

fn report(body: &[u8]) -> [u8; REPORT_LEN] {
    let mut buf = [0u8; REPORT_LEN];
    buf[1] = WRITE;
    buf[2..2 + body.len()].copy_from_slice(body);
    buf
}

/// A color message for any zone ids: for each, its id then red, green, blue.
fn zones_report(zones: &[(u8, Rgb)]) -> [u8; REPORT_LEN] {
    let mut body = vec![cmd::COLOR, zones.len() as u8, 0x01];
    for (id, c) in zones {
        body.extend_from_slice(&[*id, c.r, c.g, c.b]);
    }
    report(&body)
}

/// The button-setup message: every button, each set to report to the
/// computer if it's in `report`, otherwise left to the mouse.
fn button_setup_report(reporting: &[&str]) -> [u8; REPORT_LEN] {
    let mut body = vec![cmd::BUTTON_SETUP];
    body.extend_from_slice(&(BUTTON_IDS.len() as u16).to_le_bytes());
    for (name, id, _) in BUTTON_IDS {
        // Left always stays with the mouse, so clicking can't break.
        let mode = if name != "Left" && reporting.contains(&name) { button_mode::REPORT } else { button_mode::MOUSE };
        body.extend_from_slice(&[id, mode]);
    }
    report(&body)
}

/// Which buttons are held in a button report, by name.
pub fn buttons_held(data: &[u8]) -> Option<Vec<&'static str>> {
    if data.len() < 8 || data[0] != BUTTON_REPORT_ID {
        return None;
    }
    let mask = u16::from_le_bytes([data[1], data[4]]);
    Some(BUTTON_IDS.iter().filter(|(_, _, bit)| mask & bit != 0).map(|(n, _, _)| *n).collect())
}

/// The message setting every known zone, in [`ZONES`] order.
fn color_report(colors: &[Rgb; ZONE_COUNT]) -> [u8; REPORT_LEN] {
    let zones: Vec<(u8, Rgb)> = ZONES.iter().zip(colors).map(|((_, id), c)| (*id, *c)).collect();
    zones_report(&zones)
}

pub struct Nightsword {
    dev: HidDevice,
}

impl Nightsword {
    /// Opens the mouse's control interface and switches it to software
    /// control. The mouse keeps working normally.
    pub fn open(api: &HidApi) -> Result<Self> {
        let path = find_devices(api)
            .into_iter()
            .find(|d| d.pid == PID)
            .and_then(|d| d.interfaces.into_iter().find(|i| i.number == INTERFACE))
            .ok_or(Error::NotFound)?
            .path;
        let c_path = CString::new(path).map_err(|_| Error::NotFound)?;
        let mouse = Self { dev: api.open_path(&c_path)? };
        mouse.dev.write(&report(&[cmd::MODE, mode::SOFTWARE]))?;
        Ok(mouse)
    }

    /// Shows one screen color per zone, in [`ZONES`] order. Corrected for
    /// LEDs with [`Rgb::to_led`].
    pub fn set_colors(&self, colors: &[Rgb; ZONE_COUNT]) -> Result<()> {
        self.dev.write(&color_report(&colors.map(|c| c.to_led())))?;
        Ok(())
    }

    /// Sets colors by raw zone id (for finding zones Prism doesn't know yet).
    pub fn set_zone_ids(&self, zones: &[(u8, Rgb)]) -> Result<()> {
        let led: Vec<(u8, Rgb)> = zones.iter().map(|(id, c)| (*id, c.to_led())).collect();
        self.dev.write(&zones_report(&led))?;
        Ok(())
    }

    /// Makes the named buttons report their presses to the computer instead
    /// of the mouse handling them. Every other button goes back to normal.
    /// Not saved on the mouse: [`Nightsword::release`] puts them all back.
    pub fn report_buttons(&self, names: &[&str]) -> Result<()> {
        std::thread::sleep(GAP);
        self.dev.write(&button_setup_report(names))?;
        std::thread::sleep(GAP);
        Ok(())
    }

    /// Opens the mouse's button channel, for reading button reports.
    pub fn open_buttons(api: &HidApi) -> Result<HidDevice> {
        let path = find_devices(api)
            .into_iter()
            .find(|d| d.pid == PID)
            .and_then(|d| d.interfaces.into_iter().find(|i| i.number == BUTTON_INTERFACE))
            .ok_or(Error::NotFound)?
            .path;
        let c_path = CString::new(path).map_err(|_| Error::NotFound)?;
        Ok(api.open_path(&c_path)?)
    }

    /// Puts every button back to normal and hands lighting back to the
    /// mouse's own saved effect.
    pub fn release(&self) -> Result<()> {
        std::thread::sleep(GAP);
        self.dev.write(&button_setup_report(&[]))?;
        // The mouse misses a message sent right after another one, so give
        // it a moment first.
        std::thread::sleep(GAP);
        self.dev.write(&report(&[cmd::MODE, mode::HARDWARE]))?;
        std::thread::sleep(GAP);
        Ok(())
    }
}

/// Where each zone sits on the mouse for effects: x runs front to back,
/// y back to front (so the second direction is the first reversed).
pub fn surface() -> crate::effects::Surface {
    // front, scroll wheel, logo (palm), DPI light (left side, mid-mouse), back
    let along = [0.0, 1.0, 3.0, 2.0, 4.0];
    let pts: Vec<(f64, f64)> = along.iter().map(|&a| (a, 3.0 - a)).collect();
    crate::effects::Surface::grid(&pts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_report_layout() {
        let colors = [Rgb::new(1, 2, 3), Rgb::new(4, 5, 6), Rgb::new(7, 8, 9), Rgb::new(10, 11, 12), Rgb::new(13, 14, 15)];
        let r = color_report(&colors);
        assert_eq!(&r[..5], &[0x00, 0x07, 0x22, 0x05, 0x01]);
        // front (id 1), scroll (4), logo (2), DPI light (3), back (6)
        assert_eq!(&r[5..25], &[1, 1, 2, 3, 4, 4, 5, 6, 2, 7, 8, 9, 3, 10, 11, 12, 6, 13, 14, 15]);
        assert!(r[25..].iter().all(|&b| b == 0));
    }

    #[test]
    fn reported_buttons_have_own_codes() {
        assert_eq!(reported_button(0x2f2), Some("Sniper"));
        assert_eq!(reported_button(0x113), None); // Back is a normal Linux button
        assert!(BUTTONS.iter().all(|(n, _)| BUTTON_IDS.iter().any(|(m, _, _)| m == n)));
    }

    #[test]
    fn button_setup_layout() {
        let r = button_setup_report(&["Sniper", "Left"]);
        // write, button setup, 10 buttons (little endian)
        assert_eq!(&r[..5], &[0x00, 0x07, 0x40, 10, 0]);
        let pairs: Vec<(u8, u8)> = r[5..25].chunks(2).map(|c| (c[0], c[1])).collect();
        assert_eq!(pairs[7], (8, 0x40)); // Sniper reports
        assert_eq!(pairs[0], (1, 0xc0)); // Left never does
        assert_eq!(pairs.iter().filter(|p| p.1 == 0x40).count(), 1);
        assert_eq!(pairs[8], (25, 0xc0));
        // nothing set to report: all normal
        assert!(button_setup_report(&[])[5..25].chunks(2).all(|c| c[1] == 0xc0));
    }

    #[test]
    fn reads_button_reports() {
        let mut d = [0u8; 16];
        d[0] = 3;
        d[1] = 0x80; // Sniper
        d[4] = 0x01; // Profile Up
        assert_eq!(buttons_held(&d), Some(vec!["Sniper", "Profile Up"]));
        d[0] = 1;
        assert_eq!(buttons_held(&d), None);
    }

    #[test]
    fn mode_layout() {
        assert_eq!(&report(&[cmd::MODE, mode::SOFTWARE])[..4], &[0x00, 0x07, 0x04, 0x02]);
    }
}
