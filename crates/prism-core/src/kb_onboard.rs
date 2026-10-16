//! The K65's saved key bindings, as files in its memory: what Prism can
//! write so the keyboard keeps a profile's bindings without Prism running.
//! Pure data in and out; [`crate::k65_mini`] does the talking.
//!
//! Worked out by comparing the keyboard's files before and after iCUE saved
//! a combo and a macro (2026-10-16), plus an earlier Caps Lock -> Esc save.
//! Files are numbered; a profile is:
//! - a root file: `1e f1`, a save counter, `00 00 00`, five file numbers
//!   (key list, lighting, iCUE's index, a 4-byte "il" file, key flags),
//!   zeros up to byte 30, then the name (u16 length + text).
//! - the key list: `3d ac 00 00`, then 8 bytes per bound key:
//!   [slot, 01, 00, 00, action file (u16), kind, 00]; kind 0x61 for a key
//!   or combo, 0x31 for a macro. Slots are the lighting slots.
//! - key flags: `af 0b`, then one byte per slot: 0x3a if bound, else 0x3b.
//! - one action file per bound key:
//!   key or combo: `6e 07 01 00 00 00 00 00`, then 01 + a USB key code, or
//!   02 + several USB key codes held together.
//!   macro: `ac 30 02 00` + 12 zeros, then 2-byte steps: [01, code] press,
//!   [02, code] release, [0c, ms] wait (up to 255 ms), and `00 00` to end.

use crate::bindings::{ActionDef, BindingDef, InputDevice, MacroStep, PlayMode};
use crate::k65_mini::Key;

const ROOT_TAG: [u8; 2] = [0x1e, 0xf1];
const KEYS_TAG: [u8; 2] = [0x3d, 0xac];
const FLAGS_TAG: [u8; 2] = [0xaf, 0x0b];
const ACTION_HEAD: [u8; 8] = [0x6e, 0x07, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00];
const MACRO_HEAD: [u8; 16] = [0xac, 0x30, 0x02, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
const KIND_KEY: u8 = 0x61;
const KIND_MACRO: u8 = 0x31;
const BOUND: u8 = 0x3a;
const UNBOUND: u8 = 0x3b;
const PRESS: u8 = 0x01;
const RELEASE: u8 = 0x02;
const WAIT: u8 = 0x0c;
/// The Fn key's slot: never bound, so the Fn layer always works.
const FN_SLOT: u8 = 122;

/// Linux key code -> USB key code (HID usage), the standard table.
pub fn usb_code(code: u16) -> Option<u8> {
    let c = code as u8;
    Some(match code {
        // letters: Linux numbers them by keyboard row, USB alphabetically
        16..=25 | 30..=38 | 44..=50 => b"qwertyuiop\0\0\0\0asdfghjkl\0\0\0\0\0zxcvbnm"[(code - 16) as usize] - b'a' + 0x04,
        2..=10 => c - 2 + 0x1e, // 1-9
        11 => 0x27,             // 0
        28 => 0x28,
        1 => 0x29,
        14 => 0x2a,
        15 => 0x2b,
        57 => 0x2c,
        12 => 0x2d,
        13 => 0x2e,
        26 => 0x2f,
        27 => 0x30,
        43 => 0x31,
        39 => 0x33,
        40 => 0x34,
        41 => 0x35,
        51 => 0x36,
        52 => 0x37,
        53 => 0x38,
        58 => 0x39,
        59..=68 => c - 59 + 0x3a, // F1-F10
        87 => 0x44,
        88 => 0x45,
        99 => 0x46,
        70 => 0x47,
        119 => 0x48,
        110 => 0x49,
        102 => 0x4a,
        104 => 0x4b,
        111 => 0x4c,
        107 => 0x4d,
        109 => 0x4e,
        106 => 0x4f,
        105 => 0x50,
        108 => 0x51,
        103 => 0x52,
        69 => 0x53,
        98 => 0x54,
        55 => 0x55,
        74 => 0x56,
        78 => 0x57,
        96 => 0x58,
        79..=81 => c - 79 + 0x59, // keypad 1-3
        75..=77 => c - 75 + 0x5c, // keypad 4-6
        71..=73 => c - 71 + 0x5f, // keypad 7-9
        82 => 0x62,
        83 => 0x63,
        86 => 0x64,
        127 => 0x65,
        29 => 0xe0,
        42 => 0xe1,
        56 => 0xe2,
        125 => 0xe3,
        97 => 0xe4,
        54 => 0xe5,
        100 => 0xe6,
        126 => 0xe7,
        _ => return None,
    })
}

/// One onboard profile on the keyboard.
#[derive(Debug, Clone, PartialEq)]
pub struct Root {
    pub name: String,
    pub key_list: u16,
    pub flags: u16,
}

pub fn parse_root(f: &[u8]) -> Option<Root> {
    if f.len() < 32 || f[..2] != ROOT_TAG {
        return None;
    }
    let id = |i: usize| u16::from_le_bytes([f[6 + i * 2], f[7 + i * 2]]);
    let len = u16::from_le_bytes([f[30], f[31]]) as usize;
    let name = String::from_utf8_lossy(f.get(32..32 + len)?).into_owned();
    Some(Root { name, key_list: id(0), flags: id(4) })
}

/// The onboard profiles' root files, from file 0x000f.
pub fn profile_roots(list: &[u8]) -> Vec<u16> {
    if list.len() < 4 || list[0] != 0x13 {
        return vec![];
    }
    let n = u16::from_le_bytes([list[2], list[3]]) as usize;
    list[4..].chunks(2).take(n).filter(|c| c.len() == 2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
}

/// The action files a key list points to. None if it isn't a key list.
pub fn key_list_actions(f: &[u8]) -> Option<Vec<u16>> {
    if f.len() < 4 || f[..2] != KEYS_TAG || (f.len() - 4) % 8 != 0 {
        return None;
    }
    Some(f[4..].chunks(8).map(|e| u16::from_le_bytes([e[4], e[5]])).collect())
}

pub fn is_flags(f: &[u8]) -> bool {
    f.len() > 2 && f[..2] == FLAGS_TAG
}

/// What a key will do once saved.
#[derive(Debug, Clone, PartialEq)]
pub enum Saved {
    Key(u16),
    Combo(Vec<u16>),
    /// A macro with this many key presses.
    Macro(usize),
}

/// What saving a profile's keyboard bindings would write.
#[derive(Debug, Clone, PartialEq)]
pub struct KbPlan {
    /// (key label, what it does)
    pub saved: Vec<(&'static str, Saved)>,
    /// (key label, why not)
    pub skipped: Vec<(&'static str, &'static str)>,
    /// (slot, action file contents, kind), one per saved key.
    pub actions: Vec<(u8, Vec<u8>, u8)>,
}

/// The slot that is the key itself (the space bar has three LEDs; its
/// middle one is the key).
fn key_slot(k: &Key) -> u8 {
    usb_code(k.code).and_then(|u| k.slots.iter().copied().find(|s| *s == u)).unwrap_or(k.slots[0])
}

pub fn plan(bindings: &[BindingDef], keys: &[Key]) -> KbPlan {
    let mut plan = KbPlan { saved: vec![], skipped: vec![], actions: vec![] };
    let mut done = Vec::new();
    for b in bindings.iter().filter(|b| b.device == InputDevice::Keyboard) {
        let Some(k) = keys.iter().find(|k| k.code == b.input && k.code != 0 && !k.label.is_empty()) else { continue };
        let slot = key_slot(k);
        if slot == FN_SLOT || done.contains(&slot) {
            continue;
        }
        let why = match &b.action {
            ActionDef::Key { key } => match usb_code(*key) {
                Some(u) => {
                    plan.actions.push((slot, action_file(&[u]), KIND_KEY));
                    plan.saved.push((k.label, Saved::Key(*key)));
                    done.push(slot);
                    continue;
                }
                None => "only normal keys can be saved to the keyboard for now (not media keys or mouse buttons)",
            },
            ActionDef::Combo { keys: combo } => match combo.iter().map(|c| usb_code(*c)).collect::<Option<Vec<u8>>>() {
                Some(u) if !u.is_empty() => {
                    plan.actions.push((slot, action_file(&u), KIND_KEY));
                    plan.saved.push((k.label, Saved::Combo(combo.clone())));
                    done.push(slot);
                    continue;
                }
                _ => "a key in this combo can't be saved to the keyboard yet",
            },
            ActionDef::Macro { steps, mode } => {
                if *mode != PlayMode::Once {
                    "repeating macros need Prism running"
                } else {
                    match macro_file(steps) {
                        Some((f, presses)) => {
                            plan.actions.push((slot, f, KIND_MACRO));
                            plan.saved.push((k.label, Saved::Macro(presses)));
                            done.push(slot);
                            continue;
                        }
                        None => "this macro has a key (or a mouse button) the keyboard can't play yet",
                    }
                }
            }
            ActionDef::Launch { .. } => "opening apps needs Prism running",
            ActionDef::Dpi { .. } => "DPI changes need Prism running",
            ActionDef::Disabled => "turning a key off can't be saved to the keyboard yet",
        };
        plan.skipped.push((k.label, why));
    }
    plan
}

/// A key or combo action: one USB code, or several held together.
pub fn action_file(codes: &[u8]) -> Vec<u8> {
    let mut f = ACTION_HEAD.to_vec();
    f.push(if codes.len() == 1 { 0x01 } else { 0x02 });
    f.extend_from_slice(codes);
    f
}

/// A macro file, and how many key presses it has. Waits between key steps
/// are added up (longer ones become several wait steps); any key still held
/// at the end is released.
pub fn macro_file(steps: &[MacroStep]) -> Option<(Vec<u8>, usize)> {
    let mut out: Vec<[u8; 2]> = Vec::new();
    let mut held: Vec<u8> = Vec::new();
    let mut wait = 0u32;
    let mut presses = 0;
    let key_step = |out: &mut Vec<[u8; 2]>, step: [u8; 2], wait: &mut u32| {
        if !out.is_empty() {
            let mut left = (*wait).min(60_000);
            while left > 0 {
                let w = left.min(255);
                out.push([WAIT, w as u8]);
                left -= w;
            }
        }
        *wait = 0;
        out.push(step);
    };
    for s in steps {
        match *s {
            MacroStep::Wait { ms } => wait = wait.saturating_add(ms),
            MacroStep::Down { key } => {
                let u = usb_code(key)?;
                if !held.contains(&u) {
                    held.push(u);
                    presses += 1;
                    key_step(&mut out, [PRESS, u], &mut wait);
                }
            }
            MacroStep::Up { key } => {
                let u = usb_code(key)?;
                if let Some(i) = held.iter().position(|h| *h == u) {
                    held.remove(i);
                    key_step(&mut out, [RELEASE, u], &mut wait);
                }
            }
        }
    }
    for u in held.into_iter().rev() {
        key_step(&mut out, [RELEASE, u], &mut 0);
    }
    if presses == 0 {
        return None;
    }
    let mut f = MACRO_HEAD.to_vec();
    f.extend(out.concat());
    f.extend_from_slice(&[0, 0]);
    Some((f, presses))
}

/// The key list for `entries` (slot, action file, kind).
pub fn key_list(entries: &[(u8, u16, u8)]) -> Vec<u8> {
    let mut f = vec![KEYS_TAG[0], KEYS_TAG[1], 0, 0];
    for &(slot, id, kind) in entries {
        let [lo, hi] = id.to_le_bytes();
        f.extend_from_slice(&[slot, 0x01, 0x00, 0x00, lo, hi, kind, 0x00]);
    }
    f
}

/// A copy of the key flags file with exactly `bound` slots marked.
pub fn flags(old: &[u8], bound: &[u8]) -> Option<Vec<u8>> {
    if !is_flags(old) {
        return None;
    }
    let mut f = old.to_vec();
    for (i, b) in f[2..].iter_mut().enumerate() {
        if *b == BOUND || *b == UNBOUND {
            *b = if bound.contains(&(i as u8)) { BOUND } else { UNBOUND };
        }
    }
    Some(f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::{ActionDef, BindingDef, InputDevice, MacroStep, PlayMode};

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    fn bind(input: u16, action: ActionDef) -> BindingDef {
        BindingDef { device: InputDevice::Keyboard, input, action }
    }

    #[test]
    fn usb_codes() {
        for (linux, usb) in [(30, 0x04), (48, 0x05), (46, 0x06), (35, 0x0b), (23, 0x0c), (44, 0x1d), (16, 0x14), (2, 0x1e), (11, 0x27), (1, 0x29), (29, 0xe0), (100, 0xe6), (58, 0x39), (87, 0x44)] {
            assert_eq!(usb_code(linux), Some(usb), "linux {linux}");
        }
        assert_eq!(usb_code(113), None);
    }

    #[test]
    fn matches_icue_files() {
        let keys = crate::k65_mini::keys();
        // iCUE: Right Alt = Ctrl+C, Right Ctrl = macro H (110 ms) 251 ms I (94 ms).
        let p = plan(
            &[
                bind(100, ActionDef::Combo { keys: vec![29, 46] }),
                bind(97, ActionDef::Macro {
                    steps: vec![
                        MacroStep::Down { key: 35 }, MacroStep::Wait { ms: 110 }, MacroStep::Up { key: 35 }, MacroStep::Wait { ms: 251 },
                        MacroStep::Down { key: 23 }, MacroStep::Wait { ms: 94 }, MacroStep::Up { key: 23 },
                    ],
                    mode: PlayMode::Once,
                }),
                // Caps Lock = Esc, from the first iCUE capture
                bind(58, ActionDef::Key { key: 1 }),
            ],
            &keys,
        );
        assert_eq!(p.actions[0], (111, hex("6e0701000000000002e006"), KIND_KEY));
        assert_eq!(p.actions[1], (109, hex("ac300200000000000000000000000000010b0c6e020b0cfb010c0c5e020c0000"), KIND_MACRO));
        assert_eq!(p.actions[2], (57, hex("6e070100000000000129"), KIND_KEY));
        assert_eq!(key_list(&[(111, 0x6d63, KIND_KEY), (109, 0x6d64, KIND_MACRO)]), hex("3dac00006f010000636d61006d010000646d3100"));
        assert_eq!(key_list(&[(57, 0x6d6a, KIND_KEY)]), hex("3dac0000390100006a6d6100"));
    }

    #[test]
    fn reads_profile_files() {
        let root = hex("1ef1140000006b6d6e6d696d6d6d6c6d00000000000000000000000000000c0048572050726f66696c652032000000000000000000");
        assert_eq!(parse_root(&root), Some(Root { name: "HW Profile 2".into(), key_list: 0x6d6b, flags: 0x6d6c }));
        assert_eq!(profile_roots(&hex("13000200606d676d")), vec![0x6d60, 0x6d67]);
        assert_eq!(key_list_actions(&hex("3dac00006f010000636d61006d010000646d3100")), Some(vec![0x6d63, 0x6d64]));
        assert_eq!(key_list_actions(&hex("3dac0000")), Some(vec![]));
    }

    #[test]
    fn flags_mark_bound_keys() {
        let mut old = vec![0xaf, 0x0b];
        old.extend([UNBOUND; 123]);
        old[2 + 57] = BOUND;
        let f = flags(&old, &[109, 111]).unwrap();
        let marked: Vec<usize> = (0..123).filter(|&i| f[2 + i] == BOUND).collect();
        assert_eq!(marked, vec![109, 111]);
    }

    #[test]
    fn never_binds_fn_and_releases_keys() {
        let keys = crate::k65_mini::keys();
        let p = plan(&[bind(0, ActionDef::Key { key: 30 })], &keys);
        assert!(p.actions.is_empty());
        let (f, _) = macro_file(&[MacroStep::Down { key: 29 }, MacroStep::Wait { ms: 600 }, MacroStep::Down { key: 46 }]).unwrap();
        // press Ctrl, wait 255+255+90, press C, release C, release Ctrl, end
        assert_eq!(&f[16..], &hex("01e00cff0cff0c5a01060206 02e00000".replace(' ', "").as_str())[..]);
    }
}
