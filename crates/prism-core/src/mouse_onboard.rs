//! The Nightsword's saved settings, as files in its memory: what Prism can
//! write so the mouse keeps a profile's button keys and DPI without Prism
//! running. Pure data in and out; [`crate::nightsword`] does the talking.
//!
//! - PROFILE.MAP holds the button bindings: `41 00`, a count (u16), then
//!   3 bytes per button: [button number, key number, 0x80]. 0x80 means
//!   "acts as a keyboard key"; other kinds (media) aren't worked out yet.
//!   Button numbers count from 0 in the order of [`nightsword::BUTTONS`]
//!   (checked on the mouse: Left 0, Forward 4, DPI Down 6, Sniper 7,
//!   Profile Up 8, Profile Down 9; the rest follow the order).
//! - PROFILE.DAT attaches macros to buttons: `50 00`, a count (u16), then
//!   16 bytes per macro: [button number, macro file name ("M000"), ...].
//!   A macro wins over PROFILE.MAP, so saving a key onto a button removes
//!   any macro iCUE left on it.
//! - PROFILE.I holds the profile's name and its DPI table: 8 bytes per
//!   setting from byte 0x101, sniper first, then stages 1-3:
//!   [x u16, y u16, red, green, blue, ?]. Only the x and y numbers are
//!   changed; everything else is kept exactly as the mouse had it.

use crate::bindings::{ActionDef, BindingDef, DpiSettings, InputDevice};
use crate::nightsword;

/// Size of a PROFILE.I file.
pub const PROFILE_I_LEN: usize = 397;
const DPI_TABLE: usize = 0x101;
const DPI_ENTRY: usize = 8;
const NAME_AT: usize = 0x16;
const NAME_LEN: usize = 0xe0;
const KEY_ACTION: u8 = 0x80;

/// Linux key code -> the mouse's number for that key (its place in
/// Corsair's key list). Checked on the real mouse for Esc, 1, Q, A, Z, B,
/// C, Space and Enter; the rest follow the same list. Media keys aren't
/// here: they may need another action kind.
const KEY_NUMBERS: &[(u16, u8)] = &[
    (1, 0), (2, 13), (3, 14), (4, 15), (5, 16), (6, 17), (7, 18), (8, 19), (9, 20), (10, 21), (11, 22), (12, 23),
    (13, 84), (14, 86), (15, 24), (16, 25), (17, 26), (18, 27), (19, 28), (20, 29), (21, 30), (22, 31), (23, 32),
    (24, 33), (25, 34), (26, 35), (27, 79), (28, 82), (29, 60), (30, 37), (31, 38), (32, 39), (33, 40), (34, 41),
    (35, 42), (36, 43), (37, 44), (38, 45), (39, 46), (40, 47), (41, 12), (42, 48), (43, 80), (44, 50), (45, 51),
    (46, 52), (47, 53), (48, 54), (49, 55), (50, 56), (51, 57), (52, 58), (53, 59), (54, 90), (55, 104), (56, 62),
    (57, 64), (58, 36), (59, 1), (60, 2), (61, 3), (62, 4), (63, 5), (64, 6), (65, 7), (66, 8), (67, 9), (68, 10),
    (69, 102), (70, 74), (71, 108), (72, 109), (73, 110), (74, 105), (75, 112), (76, 113), (77, 114), (78, 106),
    (79, 115), (80, 116), (81, 117), (82, 118), (83, 119), (87, 11), (88, 72), (96, 107), (97, 91), (98, 103),
    (99, 73), (100, 67), (102, 77), (103, 92), (104, 78), (105, 93), (106, 95), (107, 88), (108, 94), (109, 89),
    (110, 76), (111, 87), (119, 75), (125, 61), (126, 68), (127, 69),
];

pub fn key_number(code: u16) -> Option<u8> {
    KEY_NUMBERS.iter().find(|(c, _)| *c == code).map(|(_, n)| *n)
}

/// What saving a profile to the mouse would write, and what it can't.
#[derive(Debug, Clone, PartialEq)]
pub struct MousePlan {
    /// (button name, key code) for each binding that will be saved.
    pub saved: Vec<(&'static str, u16)>,
    /// (button name, why) for bindings the mouse can't hold.
    pub skipped: Vec<(&'static str, &'static str)>,
    /// The new PROFILE.MAP.
    pub map: Vec<u8>,
}

/// Works out the PROFILE.MAP for a profile's mouse bindings.
pub fn plan_map(bindings: &[BindingDef]) -> MousePlan {
    let mut plan = MousePlan { saved: vec![], skipped: vec![], map: vec![] };
    let mut entries = Vec::new();
    for b in bindings.iter().filter(|b| b.device == InputDevice::Mouse) {
        let Some(id) = nightsword::BUTTONS.iter().position(|(_, c)| *c == b.input) else { continue };
        let (name, _) = nightsword::BUTTONS[id];
        let id = id as u8;
        if name == "Left" {
            continue;
        }
        let why = match &b.action {
            ActionDef::Key { key } => match key_number(*key) {
                Some(n) => {
                    entries.push([id, n, KEY_ACTION]);
                    plan.saved.push((name, *key));
                    continue;
                }
                None => "only keyboard keys can be saved for now (not mouse buttons or media keys)",
            },
            ActionDef::Combo { .. } => "key combos can't be saved to the mouse yet",
            ActionDef::Macro { .. } => "macros can't be saved to the mouse yet",
            ActionDef::Launch { .. } => "opening apps needs Prism running",
            ActionDef::Dpi { .. } => "DPI buttons need Prism running",
            ActionDef::Disabled => "turning a button off can't be saved to the mouse yet",
        };
        plan.skipped.push((name, why));
    }
    entries.sort();
    plan.map = vec![0x41, 0x00];
    plan.map.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    plan.map.extend(entries.concat());
    plan
}

const MACRO_ENTRY: usize = 16;

/// Buttons (by number) that PROFILE.DAT attaches a macro to. None if the
/// file isn't in the expected format.
pub fn macro_buttons(profile_dat: &[u8]) -> Option<Vec<u8>> {
    if profile_dat.len() < 4 || profile_dat[0] != b'P' {
        return None;
    }
    let count = u16::from_le_bytes([profile_dat[2], profile_dat[3]]) as usize;
    if profile_dat.len() != 4 + count * MACRO_ENTRY {
        return None;
    }
    Some(profile_dat[4..].chunks(MACRO_ENTRY).map(|e| e[0]).collect())
}

/// A copy of PROFILE.DAT without the macros on `buttons`. None if the file
/// isn't in the expected format.
pub fn drop_macros(profile_dat: &[u8], buttons: &[u8]) -> Option<Vec<u8>> {
    macro_buttons(profile_dat)?;
    let kept: Vec<&[u8]> = profile_dat[4..].chunks(MACRO_ENTRY).filter(|e| !buttons.contains(&e[0])).collect();
    let mut out = vec![profile_dat[0], profile_dat[1]];
    out.extend_from_slice(&(kept.len() as u16).to_le_bytes());
    out.extend(kept.concat());
    Some(out)
}

/// The button numbers a PROFILE.MAP sets.
pub fn map_buttons(map: &[u8]) -> Vec<u8> {
    map.get(4..).unwrap_or_default().chunks(3).filter(|e| e.len() == 3).map(|e| e[0]).collect()
}

/// A copy of PROFILE.I with the DPI numbers from `dpi` (sniper and stages
/// 1-3). None if the file doesn't look like a PROFILE.I.
pub fn patch_dpi(profile_i: &[u8], dpi: &DpiSettings) -> Option<Vec<u8>> {
    if profile_i.len() != PROFILE_I_LEN || profile_i[0] != b'I' {
        return None;
    }
    let dpi = dpi.clone().sanitized();
    let mut out = profile_i.to_vec();
    for (i, v) in [dpi.sniper, dpi.stages[0], dpi.stages[1], dpi.stages[2]].into_iter().enumerate() {
        let at = DPI_TABLE + i * DPI_ENTRY;
        out[at..at + 2].copy_from_slice(&v.to_le_bytes());
        out[at + 2..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    Some(out)
}

/// The DPI saved in a PROFILE.I: (sniper, [stage 1, 2, 3]), going by the
/// x numbers.
pub fn read_dpi(profile_i: &[u8]) -> Option<(u16, [u16; 3])> {
    if profile_i.len() != PROFILE_I_LEN || profile_i[0] != b'I' {
        return None;
    }
    let at = |i: usize| u16::from_le_bytes([profile_i[DPI_TABLE + i * DPI_ENTRY], profile_i[DPI_TABLE + i * DPI_ENTRY + 1]]);
    Some((at(0), [at(1), at(2), at(3)]))
}

/// The profile's name, as iCUE saved it ("Default Profile").
pub fn profile_name(profile_i: &[u8]) -> Option<String> {
    if profile_i.len() != PROFILE_I_LEN || profile_i[0] != b'I' {
        return None;
    }
    let units: Vec<u16> = profile_i[NAME_AT..NAME_AT + NAME_LEN]
        .chunks(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&u| u != 0)
        .collect();
    Some(String::from_utf16_lossy(&units))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::{ActionDef, BindingDef, InputDevice};

    fn bind(input: u16, action: ActionDef) -> BindingDef {
        BindingDef { device: InputDevice::Mouse, input, action }
    }

    #[test]
    fn key_numbers_match_the_mouse() {
        // What iCUE wrote for each key, read back from the mouse.
        for (code, n) in [(1, 0x00), (2, 0x0d), (16, 0x19), (30, 0x25), (44, 0x32), (57, 0x40), (28, 0x52), (48, 0x36), (46, 0x34)] {
            assert_eq!(key_number(code), Some(n), "key {code}");
        }
        assert_eq!(key_number(113), None); // mute
    }

    #[test]
    fn builds_the_map_like_icue() {
        // iCUE: DPI Down as Esc -> 41 00 01 00 06 00 80
        let p = plan_map(&[bind(0x2f1, ActionDef::Key { key: 1 })]);
        assert_eq!(p.map, vec![0x41, 0x00, 0x01, 0x00, 0x06, 0x00, 0x80]);
        // Forward as A, and DPI Down as Esc: sorted by button
        let p = plan_map(&[bind(0x2f1, ActionDef::Key { key: 1 }), bind(0x114, ActionDef::Key { key: 30 })]);
        assert_eq!(p.map, vec![0x41, 0x00, 0x02, 0x00, 0x04, 0x25, 0x80, 0x06, 0x00, 0x80]);
        assert_eq!(p.saved.len(), 2);
        // Tested on the mouse: Sniper 7, Profile Up 8, Profile Down 9.
        let p = plan_map(&[bind(0x2f2, ActionDef::Key { key: 8 }), bind(0x2f3, ActionDef::Key { key: 9 }), bind(0x2f4, ActionDef::Key { key: 10 })]);
        assert_eq!(map_buttons(&p.map), vec![7, 8, 9]);
    }

    #[test]
    fn skips_what_the_mouse_cant_hold() {
        let p = plan_map(&[
            bind(0x114, ActionDef::Combo { keys: vec![29, 46] }),
            bind(0x113, ActionDef::Key { key: 113 }),
            bind(0x110, ActionDef::Key { key: 30 }),
            BindingDef { device: InputDevice::Keyboard, input: 30, action: ActionDef::Key { key: 31 } },
        ]);
        assert_eq!(p.map, vec![0x41, 0x00, 0x00, 0x00]);
        assert_eq!(p.skipped.iter().map(|s| s.0).collect::<Vec<_>>(), vec!["Forward", "Back"]);
    }

    #[test]
    fn drops_icue_macros_from_saved_buttons() {
        // iCUE's PROFILE.DAT: macro M000 on button 6 (DPI Down).
        let dat = [0x50, 0x00, 0x01, 0x00, 0x06, 0x4d, 0x30, 0x30, 0x30, 0x0e, 0, 0, 0, 0x01, 0x01, 0, 0, 0, 0, 0];
        assert_eq!(macro_buttons(&dat), Some(vec![6]));
        assert_eq!(drop_macros(&dat, &[7]).as_deref(), Some(&dat[..]));
        assert_eq!(drop_macros(&dat, &[6]), Some(vec![0x50, 0x00, 0x00, 0x00]));
        assert_eq!(macro_buttons(&dat[..10]), None);
    }

    fn sample_profile_i() -> Vec<u8> {
        let mut f = vec![0u8; PROFILE_I_LEN];
        f[0] = b'I';
        for (i, ch) in "Default Profile".encode_utf16().enumerate() {
            f[NAME_AT + i * 2..NAME_AT + i * 2 + 2].copy_from_slice(&ch.to_le_bytes());
        }
        // The DPI table as read from this mouse.
        f[0xff..0x121].copy_from_slice(&[
            0x00, 0x01, 0x90, 0x01, 0x90, 0x01, 0xff, 0xff, 0x00, 0x02, 0xc1, 0x03, 0xc1, 0x03, 0x00, 0xbf, 0xff, 0x02,
            0xdc, 0x05, 0xdc, 0x05, 0x00, 0xbf, 0xff, 0x00, 0xb8, 0x0b, 0xb8, 0x0b, 0x00, 0xbf, 0xff, 0x00,
        ]);
        f
    }

    #[test]
    fn reads_and_patches_dpi() {
        let f = sample_profile_i();
        assert_eq!(profile_name(&f).as_deref(), Some("Default Profile"));
        assert_eq!(read_dpi(&f), Some((400, [961, 1500, 3000])));
        let d = DpiSettings { enabled: true, stages: [800, 1600, 3200], sniper: 450, start: 2 };
        let g = patch_dpi(&f, &d).unwrap();
        assert_eq!(read_dpi(&g), Some((450, [800, 1600, 3200])));
        // Only the DPI numbers changed.
        let changed: Vec<usize> = (0..f.len()).filter(|&i| f[i] != g[i]).collect();
        assert!(changed.iter().all(|&i| (i - DPI_TABLE) % DPI_ENTRY < 4), "{changed:?}");
        assert!(patch_dpi(&f[..10], &d).is_none());
    }
}
