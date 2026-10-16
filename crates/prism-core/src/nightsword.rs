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
    pub const DPI: u8 = 0x13;
}

/// DPI the Nightsword supports, and the step Prism uses.
pub const DPI_MIN: u16 = 100;
pub const DPI_MAX: u16 = 18000;
pub const DPI_STEP: u16 = 50;

/// Rounds a DPI to the nearest step, within the mouse's range.
pub fn clamp_dpi(dpi: u16) -> u16 {
    let d = dpi.clamp(DPI_MIN, DPI_MAX);
    ((d + DPI_STEP / 2) / DPI_STEP * DPI_STEP).clamp(DPI_MIN, DPI_MAX)
}

/// Slots in the DPI table: the sniper DPI, then stages 1-3.
const DPI_SLOT_SNIPER: u8 = 0xd0;
const DPI_SLOT_STAGE1: u8 = 0xd1;
/// Selecting a stage: 1-3, or 0 for sniper.
const DPI_SELECT: [u8; 2] = [0x02, 0x00];

/// The message defining one DPI slot: same DPI on both axes.
fn dpi_slot_report(slot: u8, dpi: u16) -> [u8; REPORT_LEN] {
    let d = clamp_dpi(dpi).to_le_bytes();
    report(&[cmd::DPI, slot, 0x00, 0x00, d[0], d[1], d[0], d[1], 0, 0, 0])
}

/// Reads ask the mouse for something; it answers on interface 0 with the
/// same first bytes.
const READ: u8 = 0x0e;
/// Onboard files (field 0x17, a small file system with 8.3 names).
mod files {
    pub const FIELD: u8 = 0x17;
    pub const ENTER: u8 = 0x0c; // pick a profile folder (no number = the first)
    pub const OPEN_READ: u8 = 0x07;
    pub const PREPARE_READ: u8 = 0x0a;
    pub const CLOSE: u8 = 0x08;
    pub const SIZE: u8 = 0x03;
    pub const WAIT: u8 = 0x0d;
    pub const PROFILES: u8 = 0x04;
    pub const BULK_READ: u8 = 0xff;
    pub const OPEN_WRITE: u8 = 0x05;
    pub const COMMIT: u8 = 0x09;
    pub const FINISH: u8 = 0x0e;
    pub const BULK_WRITE: u8 = 0x7f;
    /// Data bytes in one bulk packet, and packets per stream.
    pub const PACKET: usize = 60;
    pub const PACKETS: usize = 5;
}
/// Largest onboard file Prism will read.
const MAX_FILE: usize = 64 * 1024;
/// Names in each profile folder (lighting files are numbered after these).
const FOLDER_FILES: [&str; 5] = ["PROFILE.DAT", "PROFILE.I", "PROFILE.MAP", "lghtcnt.cnt", "M000"];

/// The message switching to a DPI stage (1-3), or sniper (0).
fn dpi_select_report(stage: u8) -> [u8; REPORT_LEN] {
    report(&[cmd::DPI, DPI_SELECT[0], DPI_SELECT[1], stage.min(3)])
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

/// A mouse that talks like the Nightsword (Corsair's older protocol).
#[derive(Debug)]
pub struct MouseModel {
    pub pid: u16,
    pub name: &'static str,
    /// Lighting zones: (name, id in the color message). Ids follow
    /// ckb-next and OpenRGB: front 1, logo 2, DPI 3, wheel 4, thumb 5,
    /// side/back 6.
    pub zones: &'static [(&'static str, u8)],
    /// Checked on a real device.
    pub tested: bool,
    /// Button reporting, DPI and onboard memory are worked out for this
    /// model. Other models get lighting only.
    pub full: bool,
}

const M65: &[(&str, u8)] = &[("Scroll wheel", 1), ("Logo", 2), ("DPI indicator", 3)];
const M65_ELITE: &[(&str, u8)] = &[("Scroll wheel", 4), ("Logo", 2), ("DPI indicator", 3)];
const SABRE: &[(&str, u8)] = &[("Front", 1), ("Scroll wheel", 4), ("Logo", 2), ("DPI indicator", 3)];
const SCIMITAR: &[(&str, u8)] = &[("Front", 1), ("Scroll wheel", 4), ("Logo", 2), ("DPI indicator", 3), ("Thumb", 5)];
const IRONCLAW: &[(&str, u8)] = &[("Scroll wheel", 4), ("Logo", 2), ("DPI indicator", 3)];
const HARPOON: &[(&str, u8)] = &[("Logo", 3)];
const GLAIVE: &[(&str, u8)] = &[("Front", 1), ("Logo", 2), ("Side lights", 6)];

const fn mouse(pid: u16, name: &'static str, zones: &'static [(&'static str, u8)]) -> MouseModel {
    MouseModel { pid, name, zones, tested: false, full: false }
}

pub const MODELS: &[MouseModel] = &[
    MouseModel { pid: PID, name: "Nightsword RGB", zones: &ZONES, tested: true, full: true },
    mouse(0x1b12, "M65 RGB", M65),
    mouse(0x1b2e, "M65 PRO RGB", M65),
    mouse(0x1b5a, "M65 RGB Elite", M65_ELITE),
    mouse(0x1b2f, "Sabre RGB", SABRE),
    mouse(0x1b32, "Sabre RGB Optical", SABRE),
    mouse(0x1b1e, "Scimitar RGB", SCIMITAR),
    mouse(0x1b3e, "Scimitar PRO RGB", SCIMITAR),
    mouse(0x1b8b, "Scimitar Elite RGB", SCIMITAR),
    mouse(0x1b5d, "Ironclaw RGB", IRONCLAW),
    mouse(0x1b3c, "Harpoon RGB", HARPOON),
    mouse(0x1b75, "Harpoon RGB PRO", HARPOON),
    mouse(0x1b34, "Glaive RGB", GLAIVE),
    mouse(0x1b74, "Glaive RGB PRO", GLAIVE),
];

pub fn model(pid: u16) -> Option<&'static MouseModel> {
    MODELS.iter().find(|m| m.pid == pid)
}

/// The first connected mouse of a model in [`MODELS`]. Read-only.
pub fn connected(api: &HidApi) -> Option<&'static MouseModel> {
    find_devices(api).iter().find_map(|d| model(d.pid))
}

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
    /// Only worked out for some models (see [`MouseModel::full`]).
    Unsupported,
    Usb(hidapi::HidError),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotFound => write!(f, "No supported Corsair mouse is connected"),
            Error::Unsupported => write!(f, "Prism can only change this mouse's lighting so far"),
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
    /// Which mouse this is (the Nightsword, or one that talks the same way).
    pub model: &'static MouseModel,
}

impl Nightsword {
    /// Opens the mouse's control interface and switches it to software
    /// control. The mouse keeps working normally.
    pub fn open(api: &HidApi) -> Result<Self> {
        Self::open_with(api, true)
    }

    /// Like [`Nightsword::open`]. With `take_control` false, the mouse is
    /// assumed to be in software mode already (Prism restarting itself), so
    /// the mode switch, which briefly blanks the lights, is skipped.
    pub fn open_with(api: &HidApi, take_control: bool) -> Result<Self> {
        let path = find_devices(api)
            .into_iter()
            .find(|d| model(d.pid).is_some())
            .ok_or(Error::NotFound)?;
        let found = model(path.pid).ok_or(Error::NotFound)?;
        let path = path.interfaces.into_iter().find(|i| i.number == INTERFACE).ok_or(Error::NotFound)?.path;
        let c_path = CString::new(path).map_err(|_| Error::NotFound)?;
        let mouse = Self { dev: api.open_path(&c_path)?, model: found };
        if take_control {
            mouse.dev.write(&report(&[cmd::MODE, mode::SOFTWARE]))?;
        }
        Ok(mouse)
    }

    /// Err(Unsupported) unless buttons, DPI and onboard memory are worked
    /// out for this model.
    fn full(&self) -> Result<()> {
        if self.model.full { Ok(()) } else { Err(Error::Unsupported) }
    }

    /// Shows one screen color per zone, in the order of this model's zones.
    /// Corrected for LEDs with [`Rgb::to_led`].
    pub fn set_zone_colors(&self, colors: &[Rgb]) -> Result<()> {
        let zones: Vec<(u8, Rgb)> = self.model.zones.iter().zip(colors).map(|((_, id), c)| (*id, c.to_led())).collect();
        self.dev.write(&zones_report(&zones))?;
        Ok(())
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
        self.full()?;
        std::thread::sleep(GAP);
        self.dev.write(&button_setup_report(names))?;
        std::thread::sleep(GAP);
        Ok(())
    }

    /// Sets the three DPI stages and the sniper DPI. Like the button setup,
    /// this is for while Prism runs; the mouse's own settings come back when
    /// it's handed back.
    pub fn set_dpi_stages(&self, stages: [u16; 3], sniper: u16) -> Result<()> {
        self.full()?;
        std::thread::sleep(GAP);
        self.dev.write(&dpi_slot_report(DPI_SLOT_SNIPER, sniper))?;
        for (i, dpi) in stages.iter().enumerate() {
            std::thread::sleep(GAP);
            self.dev.write(&dpi_slot_report(DPI_SLOT_STAGE1 + i as u8, *dpi))?;
        }
        std::thread::sleep(GAP);
        Ok(())
    }

    /// Switches to DPI stage 1-3, or 0 for the sniper DPI.
    pub fn select_dpi(&self, stage: u8) -> Result<()> {
        self.full()?;
        std::thread::sleep(GAP);
        self.dev.write(&dpi_select_report(stage))?;
        std::thread::sleep(GAP);
        Ok(())
    }

    /// Sends a message on the control interface and waits for the answer that
    /// starts with `expect`, on the answer channel `answers`.
    fn ask(&self, answers: &HidDevice, msg: &[u8], expect: &[u8]) -> Result<[u8; 64]> {
        // throw away anything already waiting
        let mut junk = [0u8; 64];
        while answers.read_timeout(&mut junk, 0).unwrap_or(0) > 0 {}
        let mut q = [0u8; REPORT_LEN];
        q[1..1 + msg.len()].copy_from_slice(msg);
        self.dev.write(&q)?;
        let end = std::time::Instant::now() + Duration::from_millis(600);
        let mut a = [0u8; 64];
        while std::time::Instant::now() < end {
            let n = answers.read_timeout(&mut a, 50)?;
            if n >= expect.len() && a[..expect.len()] == *expect {
                return Ok(a);
            }
        }
        Err(Error::NotFound)
    }

    fn tell(&self, msg: &[u8]) -> Result<()> {
        let mut q = [0u8; REPORT_LEN];
        q[1..1 + msg.len()].copy_from_slice(msg);
        self.dev.write(&q)?;
        std::thread::sleep(Duration::from_millis(12));
        Ok(())
    }

    /// Reads one onboard file by name from the current folder. Read-only.
    /// Always closes the file again, even if reading fails.
    fn read_named(&self, answers: &HidDevice, name: &str) -> Result<Vec<u8>> {
        let r = self.read_named_inner(answers, name);
        if r.is_err() {
            let _ = self.tell(&[WRITE, files::FIELD, files::CLOSE]);
        }
        r
    }

    fn read_named_inner(&self, answers: &HidDevice, name: &str) -> Result<Vec<u8>> {
        use files::*;
        let mut open = vec![WRITE, FIELD, OPEN_READ, 0x00];
        open.extend_from_slice(name.as_bytes());
        self.tell(&open)?;
        self.ask(answers, &[READ, FIELD, WAIT], &[READ, FIELD, WAIT])?;
        let size = self.ask(answers, &[READ, FIELD, SIZE, 0x01], &[READ, FIELD, SIZE, 0x01])?;
        let len = u16::from_le_bytes([size[4], size[5]]) as usize;
        if len > MAX_FILE {
            let _ = self.tell(&[WRITE, FIELD, CLOSE]);
            return Err(Error::NotFound);
        }
        let mut data = Vec::with_capacity(len);
        while data.len() < len {
            self.tell(&[WRITE, FIELD, PREPARE_READ])?;
            self.ask(answers, &[READ, FIELD, WAIT], &[READ, FIELD, WAIT])?;
            for n in 1..=PACKETS as u8 {
                if data.len() >= len {
                    break;
                }
                let take = (len - data.len()).min(PACKET) as u8;
                let a = self.ask(answers, &[BULK_READ, n, take], &[READ, BULK_READ, n, take])?;
                data.extend_from_slice(&a[4..4 + take as usize]);
            }
        }
        self.tell(&[WRITE, FIELD, CLOSE])?;
        Ok(data)
    }

    /// Replaces the file `name` in profile folder `folder` with `data`, the
    /// same steps iCUE uses: open for writing, bulk-write in streams of up to
    /// 5 x 60 bytes (each committed), close, finish. CHANGES THE MOUSE'S
    /// MEMORY: callers back up first.
    pub fn write_named(&self, api: &HidApi, folder: u8, name: &str, data: &[u8]) -> Result<()> {
        self.full()?;
        use files::*;
        if data.len() > MAX_FILE || name.is_empty() || name.len() > 12 || !name.is_ascii() {
            return Err(Error::NotFound);
        }
        let answers = Self::open_buttons(api)?;
        let enter: Vec<u8> = if folder == 0 { vec![WRITE, FIELD, ENTER] } else { vec![WRITE, FIELD, ENTER, folder] };
        self.tell(&enter)?;
        let mut open = vec![WRITE, FIELD, OPEN_WRITE, 0x00];
        open.extend_from_slice(name.as_bytes());
        self.tell(&open)?;
        let result = (|| {
            self.ask(&answers, &[READ, FIELD, WAIT], &[READ, FIELD, WAIT])?;
            for stream in data.chunks(PACKET * PACKETS) {
                for (i, packet) in stream.chunks(PACKET).enumerate() {
                    let mut msg = vec![BULK_WRITE, i as u8 + 1, packet.len() as u8, 0x00];
                    msg.extend_from_slice(packet);
                    let mut q = [0u8; REPORT_LEN];
                    q[1..1 + msg.len()].copy_from_slice(&msg);
                    self.dev.write(&q)?;
                }
                self.tell(&[WRITE, FIELD, COMMIT])?;
                self.ask(&answers, &[READ, FIELD, WAIT], &[READ, FIELD, WAIT])?;
            }
            Ok(())
        })();
        let _ = self.tell(&[WRITE, FIELD, CLOSE]);
        let _ = self.tell(&[WRITE, FIELD, FINISH]);
        let _ = self.ask(&answers, &[READ, FIELD, WAIT], &[READ, FIELD, WAIT]);
        let _ = self.tell(&[WRITE, FIELD, ENTER]);
        result
    }

    /// Reads one file by name from a profile folder. Read-only.
    pub fn read_file(&self, api: &HidApi, folder: u8, name: &str) -> Result<Vec<u8>> {
        self.full()?;
        let answers = Self::open_buttons(api)?;
        let enter: Vec<u8> =
            if folder == 0 { vec![WRITE, files::FIELD, files::ENTER] } else { vec![WRITE, files::FIELD, files::ENTER, folder] };
        self.tell(&enter)?;
        let r = self.read_named(&answers, name);
        let _ = self.tell(&[WRITE, files::FIELD, files::ENTER]);
        r
    }

    /// Reads every onboard file Prism knows the name of, from every profile
    /// folder, plus the list of profiles. Read-only: nothing on the mouse
    /// changes. Returns (folder, name, contents) for each file found.
    pub fn read_all_files(&self, api: &HidApi) -> Result<Vec<(u8, String, Vec<u8>)>> {
        self.full()?;
        use files::*;
        let answers = Self::open_buttons(api)?;
        let mut out = Vec::new();
        let list = self.ask(&answers, &[READ, FIELD, PROFILES], &[READ, FIELD, PROFILES])?;
        out.push((0, "profiles.list".to_string(), list[4..].to_vec()));
        for folder in 0..4u8 {
            let enter: Vec<u8> = if folder == 0 { vec![WRITE, FIELD, ENTER] } else { vec![WRITE, FIELD, ENTER, folder] };
            self.tell(&enter)?;
            let mut names: Vec<String> = FOLDER_FILES.iter().map(|s| s.to_string()).collect();
            // lighting files: lght_00.d / .k, lght_01.d / .k, ... as many as there are
            for i in 0..8 {
                names.push(format!("lght_{i:02}.d"));
                names.push(format!("lght_{i:02}.k"));
            }
            let mut any = false;
            let mut i = 0;
            while i < names.len() {
                let name = names[i].clone();
                i += 1;
                match self.read_named(&answers, &name) {
                    Ok(data) if !data.is_empty() => {
                        any = true;
                        // PROFILE.DAT names the macro files (M000, M001, ...).
                        if name == "PROFILE.DAT" {
                            for m in crate::mouse_onboard::macro_files(&data) {
                                if !names.contains(&m) {
                                    names.push(m);
                                }
                            }
                        }
                        out.push((folder, name, data));
                    }
                    _ => {}
                }
            }
            if !any {
                break; // no such profile folder
            }
        }
        let _ = self.tell(&[WRITE, FIELD, ENTER]);
        Ok(out)
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
    surface_for(&MODELS[0])
}

/// Where each of a model's zones sits, front to back, for effects.
pub fn surface_for(m: &MouseModel) -> crate::effects::Surface {
    let along = |name: &str| match name {
        "Front" => 0.0,
        "Scroll wheel" => 1.0,
        "DPI indicator" => 2.0,
        "Thumb" | "Side lights" => 2.5,
        "Logo" => 3.0,
        _ => 4.0, // back
    };
    let mut pts: Vec<(f64, f64)> = m.zones.iter().map(|(n, _)| (along(n), 3.0 - along(n))).collect();
    if pts.len() == 1 {
        pts.push((4.0, -1.0)); // a grid needs two points to measure; the extra is unused
        let s = crate::effects::Surface::grid(&pts);
        return crate::effects::Surface { channels: vec![1], leds: s.leds[..1].to_vec(), laps: s.laps };
    }
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
    fn dpi_messages() {
        let r = dpi_slot_report(0xd2, 1600);
        assert_eq!(&r[..13], &[0x00, 0x07, 0x13, 0xd2, 0, 0, 0x40, 0x06, 0x40, 0x06, 0, 0, 0]);
        assert_eq!(&dpi_select_report(2)[..6], &[0x00, 0x07, 0x13, 0x02, 0x00, 2]);
        assert_eq!(dpi_select_report(9)[5], 3);
        assert_eq!((clamp_dpi(20), clamp_dpi(1620), clamp_dpi(65000)), (100, 1600, 18000));
    }

    #[test]
    fn mode_layout() {
        assert_eq!(&report(&[cmd::MODE, mode::SOFTWARE])[..4], &[0x00, 0x07, 0x04, 0x02]);
    }
}
