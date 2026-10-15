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
const REPLY_TIMEOUT_MS: i32 = 30;

mod cmd {
    pub const SET: u8 = 0x01;
    pub const GET: u8 = 0x02;
    pub const CLOSE: u8 = 0x05;
    pub const WRITE: u8 = 0x06;
    pub const READ: u8 = 0x08;
    pub const PROBE: u8 = 0x09;
    pub const OPEN: u8 = 0x0d;
}

/// Handle used for reading onboard files (lighting uses [`HANDLE`]).
const FILE_HANDLE: u8 = 0x02;
/// Data bytes in one read reply (1024 minus the 3-byte header).
const READ_CHUNK: usize = 1021;
/// Largest onboard file Prism will read.
const MAX_FILE: usize = 256 * 1024;
/// Files that are always there, to start a backup from.
const ROOT_FILES: [u16; 3] = [0x0025, 0x000f, 0x0002];
/// Most files one backup will read.
const MAX_FILES: usize = 128;

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
    /// The keyboard refused a request (command, status code).
    Refused(u8, u8),
    NoReply(u8),
    TooBig(usize),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotFound => write!(f, "K65 RGB Mini is not connected"),
            Error::Usb(e) => write!(f, "USB error: {e}"),
            Error::BadSlot(s) => write!(f, "LED slot {s} does not exist (0-{})", SLOTS - 1),
            Error::Refused(c, s) => write!(f, "the keyboard refused command 0x{c:02x} (status 0x{s:02x})"),
            Error::NoReply(c) => write!(f, "the keyboard didn't answer command 0x{c:02x}"),
            Error::TooBig(n) => write!(f, "an onboard file says it's {n} bytes, which is too big to be real"),
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
        Self::open_with(api, true)
    }

    /// Like [`K65Mini::open`]. With `take_control` false, the keyboard is
    /// assumed to be in software mode already (Prism restarting itself), so
    /// the mode switch, which briefly blanks the lights, is skipped.
    pub fn open_with(api: &HidApi, take_control: bool) -> Result<Self> {
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
        if take_control {
            kb.send(&[cmd::SET, property::MODE, 0x00, mode::SOFTWARE])?;
            kb.send(&[cmd::GET, 0x6e])?;
            kb.send(&[cmd::OPEN, HANDLE, LIGHTING_RESOURCE])?;
        }
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

    /// Sends one request and waits for the keyboard's answer to it, skipping
    /// answers to other requests (lighting updates may be in flight).
    fn request(&self, body: &[u8]) -> Result<[u8; 1024]> {
        self.dev.write(&report(body))?;
        let end = std::time::Instant::now() + Duration::from_millis(500);
        let mut reply = [0u8; 1024];
        while std::time::Instant::now() < end {
            let n = self.dev.read_timeout(&mut reply, 100)?;
            if n >= 3 && reply[1] == body[0] {
                return Ok(reply);
            }
        }
        Err(Error::NoReply(body[0]))
    }

    /// Reads one onboard file. Read-only: nothing on the keyboard changes.
    pub fn read_file(&self, id: u16) -> Result<Vec<u8>> {
        let [lo, hi] = id.to_le_bytes();
        let mut r = self.request(&[cmd::OPEN, FILE_HANDLE, lo, hi])?;
        if r[2] == 0x03 {
            // already open from before: close and try again
            self.request(&[cmd::CLOSE, 0x01, FILE_HANDLE])?;
            r = self.request(&[cmd::OPEN, FILE_HANDLE, lo, hi])?;
        }
        if r[2] != 0 {
            return Err(Error::Refused(cmd::OPEN, r[2]));
        }
        let result = (|| {
            let p = self.request(&[cmd::PROBE, FILE_HANDLE])?;
            if p[2] != 0 {
                return Err(Error::Refused(cmd::PROBE, p[2]));
            }
            let len = u32::from_le_bytes([p[5], p[6], p[7], p[8]]) as usize;
            if len > MAX_FILE {
                return Err(Error::TooBig(len));
            }
            let mut data = Vec::with_capacity(len);
            while data.len() < len {
                let d = self.request(&[cmd::READ, FILE_HANDLE])?;
                if d[2] != 0 {
                    return Err(Error::Refused(cmd::READ, d[2]));
                }
                let take = (len - data.len()).min(READ_CHUNK);
                data.extend_from_slice(&d[3..3 + take]);
            }
            Ok(data)
        })();
        let _ = self.request(&[cmd::CLOSE, 0x01, FILE_HANDLE]);
        result
    }

    /// Reads every onboard file Prism can find, starting from the fixed
    /// system files and following the file numbers they mention (profile
    /// files are numbered 0x6dXX). Returns (file number, contents) pairs.
    pub fn read_all_files(&self) -> Vec<(u16, Result<Vec<u8>>)> {
        let mut todo: std::collections::VecDeque<u16> = ROOT_FILES.into_iter().collect();
        let mut seen: std::collections::BTreeSet<u16> = todo.iter().copied().collect();
        let mut out = Vec::new();
        while let Some(id) = todo.pop_front() {
            if out.len() >= MAX_FILES {
                break;
            }
            let r = self.read_file(id);
            // A number that only looked like a file reference: no such file.
            if matches!(r, Err(Error::Refused(cmd::OPEN, 0x01))) && !ROOT_FILES.contains(&id) {
                continue;
            }
            if let Ok(data) = &r {
                for w in data.windows(2) {
                    let ref_id = u16::from_le_bytes([w[0], w[1]]);
                    if w[1] == 0x6d && seen.insert(ref_id) {
                        todo.push_back(ref_id);
                    }
                }
            }
            out.push((id, r));
        }
        out
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
        self.send(&[cmd::CLOSE, 0x01, HANDLE])?;
        self.send(&[cmd::SET, property::MODE, 0x00, mode::HARDWARE])?;
        Ok(())
    }
}

/// One key on the screen: its label, the LED slots it lights, where it sits
/// (row 0 is the number row; `x` and `w` are in quarter-key units, 60
/// across), and the Linux key code it types (0 for Fn, which the keyboard
/// handles itself).
#[derive(Debug, Clone, serde::Serialize)]
pub struct Key {
    pub label: &'static str,
    pub slots: &'static [u8],
    pub row: u8,
    pub x: u8,
    pub w: u8,
    pub code: u16,
}

/// The K65 RGB Mini, US layout: (label, LED slots, width in quarter-keys,
/// Linux key code). A normal key is 4 wide. The space bar is split into its
/// three LEDs, which all type Space.
const ROWS: [&[(&str, &[u8], u8, u16)]; 5] = [
    &[
        ("Esc", &[41], 4, 1), ("1", &[30], 4, 2), ("2", &[31], 4, 3), ("3", &[32], 4, 4), ("4", &[33], 4, 5),
        ("5", &[34], 4, 6), ("6", &[35], 4, 7), ("7", &[36], 4, 8), ("8", &[37], 4, 9), ("9", &[38], 4, 10),
        ("0", &[39], 4, 11), ("-", &[45], 4, 12), ("=", &[46], 4, 13), ("Backspace", &[42], 8, 14),
    ],
    &[
        ("Tab", &[43], 6, 15), ("Q", &[20], 4, 16), ("W", &[26], 4, 17), ("E", &[8], 4, 18), ("R", &[21], 4, 19),
        ("T", &[23], 4, 20), ("Y", &[28], 4, 21), ("U", &[24], 4, 22), ("I", &[12], 4, 23), ("O", &[18], 4, 24),
        ("P", &[19], 4, 25), ("[", &[47], 4, 26), ("]", &[48], 4, 27),
        // US backslash; 50 is the same key's slot on UK boards, lit too to be safe
        ("\\", &[49, 50], 6, 43),
    ],
    &[
        ("Caps", &[57], 7, 58), ("A", &[4], 4, 30), ("S", &[22], 4, 31), ("D", &[7], 4, 32), ("F", &[9], 4, 33),
        ("G", &[10], 4, 34), ("H", &[11], 4, 35), ("J", &[13], 4, 36), ("K", &[14], 4, 37), ("L", &[15], 4, 38),
        (";", &[51], 4, 39), ("'", &[52], 4, 40), ("Enter", &[40], 9, 28),
    ],
    &[
        ("Shift", &[106], 9, 42), ("Z", &[29], 4, 44), ("X", &[27], 4, 45), ("C", &[6], 4, 46), ("V", &[25], 4, 47),
        ("B", &[5], 4, 48), ("N", &[17], 4, 49), ("M", &[16], 4, 50), (",", &[54], 4, 51), (".", &[55], 4, 52),
        ("/", &[56], 4, 53), ("Shift", &[110], 11, 54),
    ],
    &[
        ("Ctrl", &[105], 5, 29), ("Win", &[108], 5, 125), ("Alt", &[107], 5, 56),
        ("", &[0], 8, 57), ("Space", &[44], 9, 57), ("", &[1], 8, 57),
        ("Alt", &[111], 5, 100), ("Fn", &[122], 5, 0), ("Menu", &[101], 5, 127), ("Ctrl", &[109], 5, 97),
    ],
];

/// Every key, in order. A key's position in this list is its LED index in
/// lighting layers (channel 0).
pub fn keys() -> Vec<Key> {
    let mut out = Vec::new();
    for (row, keys) in ROWS.iter().enumerate() {
        let mut x = 0;
        for &(label, slots, w, code) in keys.iter() {
            out.push(Key { label, slots, row: row as u8, x, w, code });
            x += w;
        }
    }
    out
}

/// Where each key sits, for effects.
pub fn surface(keys: &[Key]) -> crate::effects::Surface {
    let pts: Vec<(f64, f64)> = keys.iter().map(|k| (k.x as f64 + k.w as f64 / 2.0, k.row as f64 * 4.0)).collect();
    crate::effects::Surface::grid(&pts)
}

/// Turns one color per key into the keyboard's slot colors.
pub fn slot_colors(keys: &[Key], colors: &[Rgb]) -> [Rgb; SLOTS] {
    let mut out = [Rgb::new(0, 0, 0); SLOTS];
    for (k, &c) in keys.iter().zip(colors) {
        for &s in k.slots {
            out[s as usize] = c;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_rows_are_full_width_and_slots_unique() {
        let keys = keys();
        for row in 0..5 {
            let width: u32 = keys.iter().filter(|k| k.row == row).map(|k| k.w as u32).sum();
            assert_eq!(width, 60, "row {row}");
        }
        let mut seen = std::collections::BTreeSet::new();
        for k in &keys {
            for &s in k.slots {
                assert!((s as usize) < SLOTS);
                assert!(seen.insert(s), "slot {s} used twice");
            }
        }
        assert_eq!(keys.len(), 63);
    }

    #[test]
    fn slot_colors_follow_keys() {
        let keys = keys();
        let mut colors = vec![Rgb::new(0, 0, 0); keys.len()];
        colors[0] = Rgb::new(9, 9, 9); // Esc
        let out = slot_colors(&keys, &colors);
        assert_eq!(out[41], Rgb::new(9, 9, 9));
        assert_eq!(out.iter().filter(|c| c.r == 9).count(), 1);
    }

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
