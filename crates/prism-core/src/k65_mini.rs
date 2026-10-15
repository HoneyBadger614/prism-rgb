//! Corsair K65 RGB Mini (1b1c:1baf).
//!
//! It uses Corsair's newer protocol. Messages are 1024-byte HID reports
//! (plus a leading report number of 0) sent to the vendor interface (usage
//! page 0xff42, usage 1), and every message starts with 0x08.
//!
//! To show our own colors we:
//!   1. switch the keyboard to software mode,
//!   2. open the lighting resource,
//!   3. write one block holding a color for every LED slot.
//!
//! LED slots are numbered by USB key code (Esc = 41, A = 4, ...). The space
//! bar has three LEDs: slots 0 (left), 44 (middle) and 1 (right).
//!
//! Without fresh colors for about a minute the keyboard falls back to its
//! own rainbow, so callers keep writing at least every few seconds.

use std::ffi::CString;
use std::time::Duration;

use hidapi::{HidApi, HidDevice};

use crate::{find_devices, Rgb};

pub const PID: u16 = 0x1baf;
/// Number of LED slots in one color block.
pub const SLOTS: usize = 123;
/// Write colors at least this often.
pub const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(4);

const VENDOR_USAGE_PAGE: u16 = 0xff42;
const VENDOR_USAGE: u16 = 0x0001;
const REPORT_LEN: usize = 1025; // report number + 1024 bytes
const HEADER: u8 = 0x08;
const REPLY_TIMEOUT_MS: i32 = 100;

mod cmd {
    pub const SET: u8 = 0x01;
    pub const GET: u8 = 0x02;
    pub const CLOSE: u8 = 0x05;
    pub const WRITE: u8 = 0x06;
    pub const OPEN: u8 = 0x0d;
}

mod property {
    pub const MODE: u8 = 0x03;
}

mod mode {
    /// The keyboard runs its own saved lighting.
    pub const HARDWARE: u8 = 0x01;
    /// The computer sends colors directly.
    pub const SOFTWARE: u8 = 0x02;
}

/// The lighting resource, and the handle we open it on.
const LIGHTING_RESOURCE: u8 = 0x22;
const HANDLE: u8 = 0x01;
/// Marks a block of plain RGB color data.
const COLOR_DATA: u8 = 0x12;

#[derive(Debug)]
pub enum Error {
    NotFound,
    Usb(hidapi::HidError),
    BadSlot(u8),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotFound => write!(f, "K65 RGB Mini is not connected"),
            Error::Usb(e) => write!(f, "USB error: {e}"),
            Error::BadSlot(s) => write!(f, "LED slot {s} does not exist (0-{})", SLOTS - 1),
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

/// Builds one outgoing report: report number 0, the 0x08 header, then `body`.
fn report(body: &[u8]) -> Vec<u8> {
    let mut buf = vec![0u8; REPORT_LEN];
    buf[1] = HEADER;
    buf[2..2 + body.len()].copy_from_slice(body);
    buf
}

/// The message holding every slot's color. `colors[slot]` is that LED's color.
fn color_report(colors: &[Rgb; SLOTS]) -> Vec<u8> {
    let len = (2 + SLOTS * 3) as u16; // data type + colors
    let mut body = vec![cmd::WRITE, HANDLE];
    body.extend_from_slice(&(len as u32).to_le_bytes());
    body.extend_from_slice(&[COLOR_DATA, 0x00]);
    for c in colors {
        body.extend_from_slice(&[c.r, c.g, c.b]);
    }
    report(&body)
}

pub struct K65Mini {
    dev: HidDevice,
}

impl K65Mini {
    /// Opens the keyboard's vendor interface and switches it to software
    /// control. Typing keeps working; only the lighting changes hands.
    pub fn open(api: &HidApi) -> Result<Self> {
        let path = find_devices(api)
            .into_iter()
            .find(|d| d.pid == PID)
            .and_then(|d| {
                d.interfaces
                    .into_iter()
                    .find(|i| i.usage_page == VENDOR_USAGE_PAGE && i.usage == VENDOR_USAGE)
            })
            .ok_or(Error::NotFound)?
            .path;
        let c_path = CString::new(path).map_err(|_| Error::NotFound)?;
        let kb = Self { dev: api.open_path(&c_path)? };
        kb.send(&[cmd::SET, property::MODE, 0x00, mode::SOFTWARE])?;
        kb.send(&[cmd::GET, 0x6e])?;
        kb.send(&[cmd::OPEN, HANDLE, LIGHTING_RESOURCE])?;
        Ok(kb)
    }

    /// Sends one message and reads (and discards) the keyboard's reply, so
    /// replies don't pile up. Returns the reply's status byte if one came.
    fn send(&self, body: &[u8]) -> Result<Option<u8>> {
        self.send_report(&report(body))
    }

    fn send_report(&self, buf: &[u8]) -> Result<Option<u8>> {
        self.dev.write(buf)?;
        let mut reply = [0u8; 1024];
        let n = self.dev.read_timeout(&mut reply, REPLY_TIMEOUT_MS)?;
        Ok((n >= 3).then_some(reply[2]))
    }

    /// Shows `colors` (screen colors, one per slot). Corrected for LEDs with
    /// [`Rgb::to_led`].
    pub fn set_colors(&self, colors: &[Rgb; SLOTS]) -> Result<()> {
        let led = colors.map(|c| c.to_led());
        self.send_report(&color_report(&led))?;
        Ok(())
    }

    /// Sets the given slots to colors and every other slot to off.
    pub fn set_slots(&self, slots: &[(u8, Rgb)]) -> Result<()> {
        let mut colors = [Rgb::new(0, 0, 0); SLOTS];
        for &(s, c) in slots {
            *colors.get_mut(s as usize).ok_or(Error::BadSlot(s))? = c;
        }
        self.set_colors(&colors)
    }

    /// Hands lighting back to the keyboard's own saved effect.
    pub fn release(&self) -> Result<()> {
        self.send(&[cmd::CLOSE, HANDLE])?;
        self.send(&[cmd::SET, property::MODE, 0x00, mode::HARDWARE])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_report_layout() {
        let mut colors = [Rgb::new(0, 0, 0); SLOTS];
        colors[0] = Rgb::new(1, 2, 3);
        colors[41] = Rgb::new(0xaa, 0xbb, 0xcc); // Esc
        let r = color_report(&colors);
        assert_eq!(r.len(), 1025);
        // report 0, header, write, handle 1, length 0x0173 (371), data type 0x12
        assert_eq!(&r[..10], &[0x00, 0x08, 0x06, 0x01, 0x73, 0x01, 0x00, 0x00, 0x12, 0x00]);
        assert_eq!(&r[10..13], &[1, 2, 3]);
        assert_eq!(&r[10 + 41 * 3..13 + 41 * 3], &[0xaa, 0xbb, 0xcc]);
        assert!(r[10 + SLOTS * 3..].iter().all(|&b| b == 0));
    }

    #[test]
    fn mode_switch_layout() {
        let r = report(&[cmd::SET, property::MODE, 0x00, mode::SOFTWARE]);
        assert_eq!(&r[..6], &[0x00, 0x08, 0x01, 0x03, 0x00, 0x02]);
    }
}
