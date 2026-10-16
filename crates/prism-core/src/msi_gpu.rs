//! MSI graphics card lighting, over the card's I2C bus.
//!
//! I2C is a small wire bus inside the card; its lighting chip sits at one
//! address on it and has numbered settings ("registers"). The card's other
//! chips (power, display) share the bus, so Prism is strict:
//! - only cards on [`SUPPORTED`] (matched by their exact PCI IDs),
//! - only the one bus and address known for them,
//! - only the lighting registers below.
//!
//! The whole card shows one color. What it was showing when Prism took over
//! is put back by [`MsiGpu::release`].

use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

use crate::Rgb;

/// (PCI vendor, device, subsystem vendor, subsystem device, name).
/// Each was checked against OpenRGB's list, which uses address 0x68 for all.
pub const SUPPORTED: &[(u16, u16, u16, u16, &str)] = &[
    (0x10de, 0x2206, 0x1462, 0x3892, "MSI RTX 3080 Gaming X Trio"),
];

const ADDRESS: u16 = 0x68;
/// NVIDIA cards expose several I2C buses; the lighting is on this one.
const BUS_NAME: &str = "NVIDIA i2c adapter 1 at ";

const REG_MODE: u8 = 0x22;
const REG_PREPARE: u8 = 0x26; // written 0 before a color change
const REG_R: u8 = 0x30;
const REG_G: u8 = 0x31;
const REG_B: u8 = 0x32;
const REG_BRIGHTNESS: u8 = 0x36; // 0-100, in steps of 20
const MODE_STATIC: u8 = 0x13;

// Linux i2c-dev interface (linux/i2c-dev.h, linux/i2c.h).
const I2C_SLAVE: libc::c_ulong = 0x0703;
const I2C_SMBUS: libc::c_ulong = 0x0720;
const I2C_SMBUS_READ: u8 = 1;
const I2C_SMBUS_WRITE: u8 = 0;
const I2C_SMBUS_BYTE_DATA: u32 = 2;

#[repr(C)]
struct SmbusData {
    block: [u8; 34],
}

#[repr(C)]
struct SmbusIoctl {
    read_write: u8,
    command: u8,
    size: u32,
    data: *mut SmbusData,
}

/// A supported card that was found (nothing has been sent to it).
#[derive(Debug, Clone)]
pub struct FoundGpu {
    pub name: &'static str,
    /// e.g. 3 for /dev/i2c-3.
    pub bus: u32,
}

/// Looks for a supported card. Read-only: only reads names in /sys.
pub fn find() -> Option<FoundGpu> {
    for entry in std::fs::read_dir("/sys/bus/i2c/devices").ok()?.flatten() {
        let file = entry.file_name();
        let Some(bus) = file.to_str().and_then(|n| n.strip_prefix("i2c-")).and_then(|n| n.parse().ok()) else { continue };
        let name = std::fs::read_to_string(entry.path().join("name")).unwrap_or_default();
        if !name.starts_with(BUS_NAME) {
            continue;
        }
        // The bus's parent folder is the graphics card's PCI device.
        let Ok(real) = std::fs::canonicalize(entry.path()) else { continue };
        let Some(pci) = real.parent() else { continue };
        let id = |f: &str| read_hex(&pci.join(f));
        let ids = (id("vendor"), id("device"), id("subsystem_vendor"), id("subsystem_device"));
        if let Some(&(.., model)) = SUPPORTED.iter().find(|(v, d, sv, sd, _)| ids == (Some(*v), Some(*d), Some(*sv), Some(*sd))) {
            return Some(FoundGpu { name: model, bus });
        }
    }
    None
}

fn read_hex(path: &Path) -> Option<u16> {
    let s = std::fs::read_to_string(path).ok()?;
    u16::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok()
}

/// What the card was showing before Prism, to put back afterwards.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Saved {
    mode: u8,
    color: [u8; 3],
    brightness: u8,
}

impl Saved {
    fn to_text(self) -> String {
        format!("{:02x} {:02x}{:02x}{:02x} {:02x}", self.mode, self.color[0], self.color[1], self.color[2], self.brightness)
    }

    fn from_text(s: &str) -> Option<Self> {
        let mut parts = s.split_whitespace();
        let byte = |t: &str| u8::from_str_radix(t, 16).ok();
        let mode = byte(parts.next()?)?;
        let c = parts.next()?;
        if c.len() != 6 {
            return None;
        }
        let color = [byte(c.get(0..2)?)?, byte(c.get(2..4)?)?, byte(c.get(4..6)?)?];
        let brightness = byte(parts.next()?)?;
        Some(Self { mode, color, brightness })
    }
}

pub struct MsiGpu {
    file: File,
    saved: Saved,
    last: Option<Rgb>,
}

/// Where the card's original look is kept while Prism has it. This survives
/// Prism restarting itself (the card is still showing Prism's colors then),
/// and is cleared by a reboot, like the card's own setting.
fn saved_path() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(|d| PathBuf::from(d).join("prism-gpu-original"))
}

impl MsiGpu {
    /// Opens the card found by [`find`] and switches it to a single steady
    /// color that Prism controls.
    pub fn open(found: &FoundGpu) -> std::io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(format!("/dev/i2c-{}", found.bus))?;
        // SAFETY: plain ioctl on our own open file descriptor.
        if unsafe { libc::ioctl(file.as_raw_fd(), I2C_SLAVE, ADDRESS as libc::c_ulong) } < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut gpu = Self { file, saved: Saved { mode: 0, color: [0; 3], brightness: 0 }, last: None };

        let from_file = saved_path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| Saved::from_text(&s));
        gpu.saved = match from_file {
            Some(s) => s,
            None => {
                let s = Saved {
                    mode: gpu.read(REG_MODE)?,
                    color: [gpu.read(REG_R)?, gpu.read(REG_G)?, gpu.read(REG_B)?],
                    brightness: gpu.read(REG_BRIGHTNESS)?,
                };
                if let Some(p) = saved_path() {
                    let _ = std::fs::write(p, s.to_text());
                }
                s
            }
        };
        if gpu.read(REG_MODE)? != MODE_STATIC {
            gpu.write(REG_MODE, MODE_STATIC)?;
        }
        if gpu.read(REG_BRIGHTNESS)? != 100 {
            gpu.write(REG_BRIGHTNESS, 100)?;
        }
        Ok(gpu)
    }

    /// Shows one color (corrected for LEDs with [`Rgb::to_led`]). Does nothing
    /// if the card already shows it.
    pub fn set_color(&mut self, color: Rgb) -> std::io::Result<()> {
        if self.last == Some(color) {
            return Ok(());
        }
        let c = color.to_led();
        self.write(REG_PREPARE, 0)?;
        self.write(REG_R, c.r)?;
        self.write(REG_G, c.g)?;
        self.write(REG_B, c.b)?;
        self.last = Some(color);
        Ok(())
    }

    /// Puts back what the card showed before Prism took over.
    pub fn release(self) -> std::io::Result<()> {
        let s = self.saved;
        self.write(REG_PREPARE, 0)?;
        self.write(REG_R, s.color[0])?;
        self.write(REG_G, s.color[1])?;
        self.write(REG_B, s.color[2])?;
        self.write(REG_BRIGHTNESS, s.brightness)?;
        self.write(REG_MODE, s.mode)?;
        if let Some(p) = saved_path() {
            let _ = std::fs::remove_file(p);
        }
        Ok(())
    }

    fn smbus(&self, read_write: u8, reg: u8, value: u8) -> std::io::Result<u8> {
        let mut data = SmbusData { block: [0; 34] };
        data.block[0] = value;
        let mut args = SmbusIoctl { read_write, command: reg, size: I2C_SMBUS_BYTE_DATA, data: &mut data };
        // SAFETY: `args` and `data` live for the whole call, laid out as the kernel expects.
        if unsafe { libc::ioctl(self.file.as_raw_fd(), I2C_SMBUS, &mut args as *mut SmbusIoctl) } < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(data.block[0])
    }

    fn read(&self, reg: u8) -> std::io::Result<u8> {
        self.smbus(I2C_SMBUS_READ, reg, 0)
    }

    fn write(&self, reg: u8, value: u8) -> std::io::Result<()> {
        debug_assert!(matches!(reg, REG_MODE | REG_PREPARE | REG_R | REG_G | REG_B | REG_BRIGHTNESS));
        self.smbus(I2C_SMBUS_WRITE, reg, value).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_look_round_trips() {
        let s = Saved { mode: 0x13, color: [0x42, 0x01, 0x63], brightness: 100 };
        assert_eq!(s.to_text(), "13 420163 64");
        assert_eq!(Saved::from_text(&s.to_text()), Some(s));
        assert_eq!(Saved::from_text("junk"), None);
        assert_eq!(Saved::from_text("13 4201 64"), None);
    }
}
