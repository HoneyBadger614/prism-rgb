//! Corsair Lighting Node Pro (1b1c:0c0b) and the controllers that talk the
//! same way: Lighting Node Core, Commander Pro (its lighting), LS100,
//! LT100 and the 1000D and SPEC-OMEGA cases. Only the Lighting Node Pro has
//! been tested; the others follow OpenRGB, which drives them all alike.
//!
//! How it talks: every message is a 64-byte HID report (plus a leading report
//! number of 0). The first byte is the command. Replies are 16 bytes.
//!
//! To show our own colors on a channel we:
//!   1. switch the channel to software control,
//!   2. send the red, green and blue values (up to 50 LEDs per message),
//!   3. send "commit" so the device displays them.
//!
//! If the device hears nothing for a while it falls back to its own built-in
//! effect, so callers must send [`LightingNode::keepalive`] every few seconds.

use std::cell::Cell;
use std::time::{Duration, Instant};

use hidapi::{HidApi, HidDevice};

use crate::{find_devices, Rgb};

pub const PID: u16 = 0x0c0b;
/// Most channels any of these controllers has.
pub const CHANNELS: u8 = 2;

/// (USB product ID, name, lighting channels, tested on a real device).
pub const MODELS: &[(u16, &str, u8, bool)] = &[
    (0x0c0b, "Lighting Node Pro", 2, true),
    (0x0c1a, "Lighting Node Core", 1, false),
    (0x0c10, "Commander Pro", 2, false),
    (0x0c1e, "LS100 Lighting Kit", 1, false),
    (0x0c23, "LT100 Smart Lighting Towers", 2, false),
    (0x1d00, "Obsidian 1000D", 2, false),
    (0x1d04, "SPEC-OMEGA RGB", 2, false),
];

pub fn model(pid: u16) -> Option<(u16, &'static str, u8, bool)> {
    MODELS.iter().copied().find(|m| m.0 == pid)
}
/// Most LEDs one channel can drive.
pub const MAX_LEDS_PER_CHANNEL: usize = 204;
/// Send a keepalive at least this often.
pub const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(4);

const REPORT_LEN: usize = 65; // report number + 64 bytes
const REPLY_LEN: usize = 16;
const REPLY_TIMEOUT_MS: i32 = 100;
const LEDS_PER_MESSAGE: usize = 50;

mod cmd {
    pub const FIRMWARE: u8 = 0x02;
    pub const DIRECT: u8 = 0x32;
    pub const COMMIT: u8 = 0x33;
    pub const PORT_STATE: u8 = 0x38;
    pub const BRIGHTNESS: u8 = 0x39;
}

mod port_state {
    /// The device runs its own saved effect.
    pub const HARDWARE: u8 = 0x01;
    /// The computer sends colors directly.
    pub const SOFTWARE: u8 = 0x02;
}

mod color_part {
    pub const RED: u8 = 0x00;
    pub const GREEN: u8 = 0x01;
    pub const BLUE: u8 = 0x02;
}

#[derive(Debug)]
pub enum Error {
    NotFound,
    Usb(hidapi::HidError),
    NoReply(u8),
    Rejected { command: u8, status: u8 },
    BadChannel(u8),
    TooManyLeds(usize),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotFound => write!(f, "No Corsair lighting controller is connected"),
            Error::Usb(e) => write!(f, "USB error: {e}"),
            Error::NoReply(c) => write!(f, "the Lighting Node Pro did not answer command 0x{c:02x}"),
            Error::Rejected { command, status } => {
                write!(f, "the Lighting Node Pro rejected command 0x{command:02x} (status 0x{status:02x})")
            }
            Error::BadChannel(c) => write!(f, "channel {} does not exist on this controller", c + 1),
            Error::TooManyLeds(n) => write!(f, "{n} LEDs is more than one channel supports ({MAX_LEDS_PER_CHANNEL})"),
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

/// Builds one outgoing report: report number 0, the command, then its arguments.
fn report(command: u8, args: &[u8]) -> [u8; REPORT_LEN] {
    let mut buf = [0u8; REPORT_LEN];
    buf[1] = command;
    buf[2..2 + args.len()].copy_from_slice(args);
    buf
}

/// The messages that set one channel's LEDs: three per batch of 50 LEDs
/// (red values, green values, blue values).
fn direct_reports(channel: u8, colors: &[Rgb]) -> Vec<[u8; REPORT_LEN]> {
    let mut out = Vec::new();
    for (batch, chunk) in colors.chunks(LEDS_PER_MESSAGE).enumerate() {
        let start = (batch * LEDS_PER_MESSAGE) as u8;
        for (part, pick) in [
            (color_part::RED, (|c: &Rgb| c.r) as fn(&Rgb) -> u8),
            (color_part::GREEN, |c: &Rgb| c.g),
            (color_part::BLUE, |c: &Rgb| c.b),
        ] {
            let mut args = vec![channel, start, chunk.len() as u8, part];
            args.extend(chunk.iter().map(pick));
            out.push(report(cmd::DIRECT, &args));
        }
    }
    out
}

pub struct LightingNode {
    dev: HidDevice,
    /// How many lighting channels this model has.
    channels: u8,
    /// When each channel was last switched to software control. Re-sent
    /// every keepalive interval in case the device dropped back on its own.
    software_since: Cell<[Option<Instant>; CHANNELS as usize]>,
}

impl LightingNode {
    /// Opens the first connected controller of a model in [`MODELS`].
    pub fn open(api: &HidApi) -> Result<Self> {
        let dev = find_devices(api).into_iter().find(|d| model(d.pid).is_some()).ok_or(Error::NotFound)?;
        let channels = model(dev.pid).map_or(1, |m| m.2);
        let path = dev.interfaces.into_iter().next().ok_or(Error::NotFound)?.path;
        let c_path = std::ffi::CString::new(path).map_err(|_| Error::NotFound)?;
        Ok(Self { dev: api.open_path(&c_path)?, channels, software_since: Cell::new([None; CHANNELS as usize]) })
    }

    pub fn channels(&self) -> u8 {
        self.channels
    }

    /// Treats both channels as already in software mode (Prism restarting
    /// itself), so the first colors don't re-send the mode switch, which
    /// briefly blanks the lights. It's re-sent at the next keepalive anyway.
    pub fn assume_software(&self) {
        self.software_since.set([Some(Instant::now()); CHANNELS as usize]);
    }

    /// Sends one command and waits for the device's reply.
    fn send(&self, command: u8, args: &[u8]) -> Result<[u8; REPLY_LEN]> {
        self.send_report(&report(command, args))
    }

    fn send_report(&self, buf: &[u8; REPORT_LEN]) -> Result<[u8; REPLY_LEN]> {
        self.dev.write(buf)?;
        let mut reply = [0u8; REPLY_LEN];
        let n = self.dev.read_timeout(&mut reply, REPLY_TIMEOUT_MS)?;
        if n == 0 {
            return Err(Error::NoReply(buf[1]));
        }
        Ok(reply)
    }

    /// Reads the firmware version, e.g. (0, 10, 4). A safe "are you there?" check.
    pub fn firmware_version(&self) -> Result<(u8, u8, u8)> {
        let r = self.send(cmd::FIRMWARE, &[])?;
        Ok((r[1], r[2], r[3]))
    }

    /// Shows `colors` (screen colors) on a channel (0 or 1). LED 1 is the
    /// first entry. Colors are corrected for LEDs with [`Rgb::to_led`].
    pub fn set_colors(&self, channel: u8, colors: &[Rgb]) -> Result<()> {
        if channel >= self.channels {
            return Err(Error::BadChannel(channel));
        }
        if colors.len() > MAX_LEDS_PER_CHANNEL {
            return Err(Error::TooManyLeds(colors.len()));
        }
        let led: Vec<Rgb> = colors.iter().map(|c| c.to_led()).collect();
        let mut since = self.software_since.get();
        let slot = &mut since[channel as usize];
        if slot.map_or(true, |t| t.elapsed() >= KEEPALIVE_INTERVAL) {
            self.checked(cmd::PORT_STATE, &[channel, port_state::SOFTWARE])?;
            *slot = Some(Instant::now());
            self.software_since.set(since);
        }
        for r in direct_reports(channel, &led) {
            self.send_report(&r)?;
        }
        self.commit()
    }

    /// Sets brightness for a channel, 0–100 percent.
    pub fn set_brightness(&self, channel: u8, percent: u8) -> Result<()> {
        if channel >= self.channels {
            return Err(Error::BadChannel(channel));
        }
        self.checked(cmd::BRIGHTNESS, &[channel, percent.min(100)])
    }

    /// Tells the device we're still here so it keeps showing our colors.
    pub fn keepalive(&self) -> Result<()> {
        self.commit()
    }

    /// Hands both channels back to the device's own saved effect.
    pub fn release(&self) -> Result<()> {
        for ch in 0..self.channels {
            self.checked(cmd::PORT_STATE, &[ch, port_state::HARDWARE])?;
        }
        self.software_since.set([None; CHANNELS as usize]);
        self.commit()
    }

    fn commit(&self) -> Result<()> {
        self.checked(cmd::COMMIT, &[0xff])
    }

    /// Like `send`, but treats a non-zero status byte as a refusal.
    fn checked(&self, command: u8, args: &[u8]) -> Result<()> {
        let r = self.send(command, args)?;
        match r[0] {
            0 => Ok(()),
            status => Err(Error::Rejected { command, status }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_layout() {
        let r = report(cmd::PORT_STATE, &[1, port_state::SOFTWARE]);
        assert_eq!(r.len(), 65);
        assert_eq!(&r[..4], &[0x00, 0x38, 0x01, 0x02]);
        assert!(r[4..].iter().all(|&b| b == 0));
    }

    #[test]
    fn direct_reports_split_into_batches_of_50() {
        let colors = vec![Rgb::new(10, 20, 30); 60];
        let reps = direct_reports(1, &colors);
        assert_eq!(reps.len(), 6); // 2 batches x (R, G, B)

        // first batch, red: channel 1, start 0, 50 LEDs, part red, then values
        assert_eq!(&reps[0][1..6], &[0x32, 1, 0, 50, color_part::RED]);
        assert!(reps[0][6..56].iter().all(|&b| b == 10));
        assert_eq!(reps[0][56], 0);

        // second batch, blue: start 50, 10 LEDs
        assert_eq!(&reps[5][1..6], &[0x32, 1, 50, 10, color_part::BLUE]);
        assert!(reps[5][6..16].iter().all(|&b| b == 30));
        assert_eq!(reps[5][16], 0);
    }

    #[test]
    fn full_channel_fits() {
        let colors = vec![Rgb::new(1, 2, 3); MAX_LEDS_PER_CHANNEL];
        let reps = direct_reports(0, &colors);
        assert_eq!(reps.len(), 15); // 5 batches x 3
        assert_eq!(reps[14][3], 200); // last batch starts at LED 200
        assert_eq!(reps[14][4], 4);
    }
}
