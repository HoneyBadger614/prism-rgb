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
}

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

    /// Hands lighting back to the mouse's own saved effect.
    pub fn release(&self) -> Result<()> {
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
    fn mode_layout() {
        assert_eq!(&report(&[cmd::MODE, mode::SOFTWARE])[..4], &[0x00, 0x07, 0x04, 0x02]);
    }
}
