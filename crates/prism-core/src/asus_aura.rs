//! ASUS Aura motherboard lighting (the board's USB "AURA LED Controller").
//!
//! The board has some built-in lights (on the board itself and its 4-pin RGB
//! sockets) and ARGB headers: sockets for lights that can be colored one LED
//! at a time, like a CPU cooler. The controller says how many of each it has,
//! but not how many LEDs are plugged into an ARGB header; that's a setting.
//!
//! Reports are 65 bytes and always start with 0xEC.

use std::time::Duration;

use hidapi::{HidApi, HidDevice};

use crate::Rgb;

pub const ASUS_VID: u16 = 0x0b05;
/// Product IDs checked to work (others may use the same protocol, untested).
pub const SUPPORTED_PIDS: &[u16] = &[0x1939];
/// Most LEDs Prism will drive on one ARGB header.
pub const MAX_ARGB_LEDS: usize = 120;

const REPORT: u8 = 0xEC;
const REQ_CONFIG: u8 = 0xB0; // reply 0x30: the 60-byte config table
const REQ_FIRMWARE: u8 = 0x82; // reply 0x02: firmware name
const SET_EFFECT: u8 = 0x35;
const DIRECT: u8 = 0x40;
const MODE_DIRECT: u8 = 0xFF;
const APPLY: u8 = 0x80;
const FIXED_CHANNEL: u8 = 0x04;
const LEDS_PER_PACKET: usize = 20;

/// Whether an ASUS Aura motherboard controller is plugged in. Read-only.
pub fn present(api: &HidApi) -> bool {
    api.device_list().any(|d| d.vendor_id() == ASUS_VID && SUPPORTED_PIDS.contains(&d.product_id()))
}

/// What the controller has, from its config table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    /// Built-in lights, driven together as one channel.
    pub fixed: usize,
    pub argb_headers: usize,
}

impl Layout {
    fn from_table(t: &[u8; 60]) -> Self {
        // 0x1B counts every built-in light, the 4-pin RGB sockets (0x1D) included.
        Self { fixed: t[0x1B] as usize, argb_headers: t[0x02] as usize }
    }
}

pub struct AsusAura {
    dev: HidDevice,
    pub layout: Layout,
    pub firmware: String,
    /// What each channel was last sent, to skip repeats. Index 0 = built-in,
    /// then each ARGB header.
    last: Vec<Vec<Rgb>>,
    /// How many LEDs each ARGB header was last sent (including blanking), so
    /// LEDs dropped from the count get turned off.
    sent_len: Vec<usize>,
}

impl AsusAura {
    /// Opens the controller, reads what it has and switches every channel
    /// to colors from Prism.
    pub fn open(api: &HidApi) -> Result<Self, String> {
        let info = api
            .device_list()
            .find(|d| d.vendor_id() == ASUS_VID && SUPPORTED_PIDS.contains(&d.product_id()))
            .ok_or("No ASUS Aura motherboard controller found")?;
        let dev = info.open_device(api).map_err(|e| format!("Could not open the ASUS Aura controller: {e}"))?;
        let mut aura = Self { dev, layout: Layout { fixed: 0, argb_headers: 0 }, firmware: String::new(), last: vec![], sent_len: vec![] };

        let fw = aura.ask(REQ_FIRMWARE, 0x02)?;
        aura.firmware = String::from_utf8_lossy(&fw[2..18]).trim_end_matches('\0').to_string();
        let reply = aura.ask(REQ_CONFIG, 0x30)?;
        let mut table = [0u8; 60];
        table.copy_from_slice(&reply[4..64]);
        aura.layout = Layout::from_table(&table);
        aura.last = vec![vec![]; 1 + aura.layout.argb_headers];
        // Earlier sessions may have lit more LEDs than the count now set.
        aura.sent_len = vec![60; aura.layout.argb_headers];

        // The same start-up message OpenRGB sends, then direct mode everywhere.
        aura.send(&[0x52, 0x53, 0x00, 0x01])?;
        let channels = (aura.layout.fixed > 0) as usize + aura.layout.argb_headers;
        for effect_channel in 0..channels {
            aura.send(&[SET_EFFECT, effect_channel as u8, 0x00, 0x00, MODE_DIRECT])?;
        }
        Ok(aura)
    }

    /// Sets the built-in lights and each ARGB header's LEDs. Colors are
    /// corrected for LEDs with [`Rgb::to_led`]; unchanged channels are skipped.
    pub fn set_colors(&mut self, fixed: &[Rgb], argb: &[Vec<Rgb>]) -> Result<(), String> {
        if self.layout.fixed > 0 && self.last[0] != fixed {
            let mut led: Vec<Rgb> = fixed.iter().map(|c| c.to_led()).collect();
            led.resize(self.layout.fixed, Rgb::new(0, 0, 0));
            self.direct(FIXED_CHANNEL, &led)?;
            self.last[0] = fixed.to_vec();
        }
        for (h, colors) in argb.iter().enumerate().take(self.layout.argb_headers) {
            let colors = &colors[..colors.len().min(MAX_ARGB_LEDS)];
            if self.last[h + 1] == colors {
                continue;
            }
            let mut led: Vec<Rgb> = colors.iter().map(|c| c.to_led()).collect();
            led.resize(colors.len().max(self.sent_len[h]), Rgb::new(0, 0, 0));
            if !led.is_empty() {
                self.direct(h as u8, &led)?;
            }
            self.sent_len[h] = colors.len();
            self.last[h + 1] = colors.to_vec();
        }
        Ok(())
    }

    /// Colors for one channel, 20 LEDs per report; the last report applies them.
    fn direct(&self, channel: u8, colors: &[Rgb]) -> Result<(), String> {
        let mut done = 0;
        while done < colors.len() {
            let n = (colors.len() - done).min(LEDS_PER_PACKET);
            let mut cmd = channel;
            if done + n >= colors.len() {
                cmd |= APPLY;
            }
            let mut msg = vec![DIRECT, cmd, done as u8, n as u8]; // done < MAX_ARGB_LEDS
            for c in &colors[done..done + n] {
                msg.extend([c.r, c.g, c.b]);
            }
            self.send(&msg)?;
            done += n;
        }
        Ok(())
    }

    fn send(&self, body: &[u8]) -> Result<(), String> {
        let mut buf = [0u8; 65];
        buf[0] = REPORT;
        buf[1..1 + body.len()].copy_from_slice(body);
        self.dev.write(&buf).map(|_| ()).map_err(|e| format!("ASUS Aura controller: {e}"))
    }

    /// Sends a request and waits for its reply (identified by byte 1).
    fn ask(&self, req: u8, reply: u8) -> Result<[u8; 65], String> {
        // Drop anything left over from before.
        let mut buf = [0u8; 65];
        while self.dev.read_timeout(&mut buf, 0).unwrap_or(0) > 0 {}
        self.send(&[req])?;
        let until = std::time::Instant::now() + Duration::from_millis(500);
        while std::time::Instant::now() < until {
            let mut buf = [0u8; 65];
            let n = self.dev.read_timeout(&mut buf, 100).map_err(|e| format!("ASUS Aura controller: {e}"))?;
            if n > 1 && buf[1] == reply {
                return Ok(buf);
            }
        }
        Err("The ASUS Aura controller didn't answer".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_layout_from_config_table() {
        // This PC's controller (firmware AULA3-AR42-0207).
        let mut t = [0u8; 60];
        t[..12].copy_from_slice(&[0x1e, 0x9f, 0x01, 0x01, 0, 0, 0x78, 0x3c, 0, 0, 0, 0]);
        t[24..30].copy_from_slice(&[0, 0, 0, 0x05, 0x06, 0x02]);
        assert_eq!(Layout::from_table(&t), Layout { fixed: 5, argb_headers: 1 });
    }
}
