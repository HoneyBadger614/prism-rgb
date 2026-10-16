//! The Nightsword's saved settings, as files in its memory: what Prism can
//! write so the mouse keeps a profile's button keys and DPI without Prism
//! running. Pure data in and out; [`crate::nightsword`] does the talking.
//!
//! - PROFILE.MAP holds single-key bindings: `41 00`, a count (u16), then
//!   3 bytes per button: [button number, key number, 0x80]. Key numbers are
//!   places in Corsair's key list; media keys and mouse buttons are in it too.
//!   Button numbers count from 0 in the order of [`nightsword::BUTTONS`]
//!   (checked on the mouse: Left 0, Forward 4, DPI Up 5, DPI Down 6,
//!   Sniper 7, Profile Up 8, Profile Down 9; the rest follow the order).
//! - PROFILE.DAT attaches macro files to buttons: `50 00`, a count (u16),
//!   then 16 bytes each: [button number, file name ("M000"), file size
//!   (u32), 01 01 (play once), zeros]. Key combos are saved as macros.
//!   Saving replaces the whole button setup, iCUE's leftovers included.
//! - PROFILE.I holds the profile's name and its DPI table: 8 bytes per
//!   setting from byte 0x101, sniper first, then stages 1-3:
//!   [x u16, y u16, red, green, blue, ?]. Only the x and y numbers are
//!   changed; everything else is kept exactly as the mouse had it.

use crate::bindings::{ActionDef, BindingDef, DpiSettings, InputDevice, MacroStep, PlayMode};
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
/// C, Space and Enter; the rest follow the same list.
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
    // Media keys, from the same list (Volume Up checked with iCUE).
    (113, 0x61), (114, 0x83), (115, 0x82), (163, 0x65), (164, 0x64), (165, 0x63), (166, 0x62),
];

pub fn key_number(code: u16) -> Option<u8> {
    KEY_NUMBERS.iter().find(|(c, _)| *c == code).map(|(_, n)| *n)
}

/// What a button will do once saved.
#[derive(Debug, Clone, PartialEq)]
pub enum Saved {
    Key(u16),
    Combo(Vec<u16>),
    /// A macro with this many key presses.
    Macro(usize),
}

/// What saving a profile to the mouse would write, and what it can't.
#[derive(Debug, Clone, PartialEq)]
pub struct MousePlan {
    pub saved: Vec<(&'static str, Saved)>,
    /// (button name, why) for bindings the mouse can't hold.
    pub skipped: Vec<(&'static str, &'static str)>,
    /// The new PROFILE.MAP (single keys and mouse buttons).
    pub map: Vec<u8>,
    /// The new PROFILE.DAT (which button plays which macro file).
    pub dat: Vec<u8>,
    /// Macro files to write: ("M000", contents). Combos are saved as
    /// macros too, the way iCUE does it.
    pub macros: Vec<(String, Vec<u8>)>,
}

/// Mouse buttons a button can act as, as numbers in PROFILE.MAP (after the
/// keyboard keys in Corsair's list). All checked with iCUE: Left 0xc8,
/// Right 0xc9, Middle 0xca, Back 0xcb, Forward 0xcc.
const MOUSE_NUMBERS: &[(u16, u8)] = &[(0x110, 0xc8), (0x111, 0xc9), (0x112, 0xca), (0x113, 0xcb), (0x114, 0xcc)];

/// Works out every file for a profile's mouse bindings. Everything the
/// mouse can't hold is listed in `skipped`.
pub fn plan_map(bindings: &[BindingDef]) -> MousePlan {
    let mut plan = MousePlan { saved: vec![], skipped: vec![], map: vec![], dat: vec![], macros: vec![] };
    let mut entries = Vec::new();
    let mut macro_entries = Vec::new();
    for b in bindings.iter().filter(|b| b.device == InputDevice::Mouse) {
        let Some(id) = nightsword::BUTTONS.iter().position(|(_, c)| *c == b.input) else { continue };
        let (name, _) = nightsword::BUTTONS[id];
        let id = id as u8;
        if name == "Left" {
            continue;
        }
        let mut add_macro = |plan: &mut MousePlan, file: Vec<u8>, saved: Saved| {
            let file_name = format!("M{:03}", plan.macros.len());
            macro_entries.push((id, file_name.clone(), file.len()));
            plan.macros.push((file_name, file));
            plan.saved.push((name, saved));
        };
        let why = match &b.action {
            ActionDef::Key { key } => {
                match key_number(*key).or_else(|| MOUSE_NUMBERS.iter().find(|(c, _)| c == key).map(|(_, n)| *n)) {
                    Some(n) => {
                        entries.push([id, n, KEY_ACTION]);
                        plan.saved.push((name, Saved::Key(*key)));
                        continue;
                    }
                    None => "this key or mouse button can't be saved to the mouse yet",
                }
            }
            ActionDef::Combo { keys } => match combo_file(keys) {
                Some(f) => {
                    add_macro(&mut plan, f, Saved::Combo(keys.clone()));
                    continue;
                }
                None => "a key in this combo can't be saved to the mouse yet",
            },
            ActionDef::Macro { steps, mode } => {
                if *mode != PlayMode::Once {
                    "repeating macros need Prism running"
                } else {
                    match macro_file(steps) {
                        Some((f, presses)) => {
                            add_macro(&mut plan, f, Saved::Macro(presses));
                            continue;
                        }
                        None => "this macro has a key (or a mouse button) the mouse can't play yet",
                    }
                }
            }
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

    plan.dat = vec![b'P', 0x00];
    plan.dat.extend_from_slice(&(macro_entries.len() as u16).to_le_bytes());
    for (button, file, size) in macro_entries {
        let mut e = vec![button];
        e.extend_from_slice(file.as_bytes());
        e.extend_from_slice(&(size as u32).to_le_bytes());
        // Play once (as iCUE writes it).
        e.extend_from_slice(&[0x01, 0x01]);
        e.resize(MACRO_ENTRY, 0);
        plan.dat.extend(e);
    }
    plan
}

/* ---------- macro files ----------
 * "M" kind 00 00, then a count (u32) of 2-byte steps:
 *   [0x20, key]   press a key (key = its number, as in PROFILE.MAP)
 *   [0x00, key]   release it
 *   [0x80 | hi, lo] wait (hi << 8 | lo) milliseconds
 * Key steps and waits take turns. Kind is 1 for a key combo, 0 for a macro.
 */

const PRESS: u8 = 0x20;
const RELEASE: u8 = 0x00;
const MAX_STEP_WAIT: u32 = 0x7fff;

fn wait_step(ms: u32) -> [u8; 2] {
    let ms = ms.min(MAX_STEP_WAIT);
    [0x80 | (ms >> 8) as u8, (ms & 0xff) as u8]
}

fn file_from_steps(kind: u8, steps: &[[u8; 2]]) -> Vec<u8> {
    let mut f = vec![b'M', kind, 0x00, 0x00];
    f.extend_from_slice(&(steps.len() as u32).to_le_bytes());
    f.extend(steps.concat());
    f
}

/// A key combo as iCUE saves it: each key pressed in turn, then released
/// in reverse, with no waits.
pub fn combo_file(keys: &[u16]) -> Option<Vec<u8>> {
    let nums: Vec<u8> = keys.iter().map(|k| key_number(*k)).collect::<Option<_>>()?;
    if nums.is_empty() {
        return None;
    }
    let mut steps = Vec::new();
    for &n in nums.iter().chain(nums.iter().rev()) {
        if !steps.is_empty() {
            steps.push(wait_step(0));
        }
        let pressed = steps.iter().filter(|s: &&[u8; 2]| s[1] == n && s[0] == PRESS).count() > 0;
        steps.push([if pressed { RELEASE } else { PRESS }, n]);
    }
    Some(file_from_steps(0x01, &steps))
}

/// A macro file for Prism's recorded steps, and how many key presses it
/// has. Waits between two key steps are added up; any key still held at
/// the end is released, so nothing can get stuck down.
pub fn macro_file(steps: &[MacroStep]) -> Option<(Vec<u8>, usize)> {
    let mut out: Vec<[u8; 2]> = Vec::new();
    let mut held: Vec<u8> = Vec::new();
    let mut wait = 0u32;
    let mut presses = 0;
    let key_step = |out: &mut Vec<[u8; 2]>, step: [u8; 2], wait: &mut u32| {
        if !out.is_empty() {
            out.push(wait_step(*wait));
        }
        *wait = 0;
        out.push(step);
    };
    for s in steps {
        match *s {
            MacroStep::Wait { ms } => wait = wait.saturating_add(ms),
            MacroStep::Down { key } => {
                let n = key_number(key)?;
                if !held.contains(&n) {
                    held.push(n);
                    presses += 1;
                    key_step(&mut out, [PRESS, n], &mut wait);
                }
            }
            MacroStep::Up { key } => {
                let n = key_number(key)?;
                if let Some(i) = held.iter().position(|h| *h == n) {
                    held.remove(i);
                    key_step(&mut out, [RELEASE, n], &mut wait);
                }
            }
        }
    }
    for n in held.into_iter().rev() {
        key_step(&mut out, [RELEASE, n], &mut 0);
    }
    if presses == 0 {
        return None;
    }
    Some((file_from_steps(0x00, &out), presses))
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

/// The macro file names PROFILE.DAT points to ("M000", "M001", ...).
pub fn macro_files(profile_dat: &[u8]) -> Vec<String> {
    if macro_buttons(profile_dat).is_none() {
        return vec![];
    }
    profile_dat[4..]
        .chunks(MACRO_ENTRY)
        .filter_map(|e| {
            let n = &e[1..5];
            n.iter().all(u8::is_ascii_alphanumeric).then(|| String::from_utf8_lossy(n).into_owned())
        })
        .collect()
}

/* ---------- lighting ----------
 * lghtcnt.cnt: how many lighting entries (u32). Entry n is two files:
 *   lght_nn.d, 13 bytes, as iCUE writes them:
 *     steady:  07 00 02 00 00, red, green, blue, ff, 00 00 00 00
 *     rainbow: 03 02 00, then zeros
 *     pulse:   01 02 03 00 00, red, green, blue, ff, red, green, blue, ff
 *   lght_nn.k: a count (u32), then the zones it lights.
 * Zones (checked on the mouse): Logo 0, Scroll wheel 2, Front 3, Back 5.
 * Zone 1 is the DPI light, which always shows the DPI stage's color.
 */

/// Prism's zone name -> the mouse's onboard zone number.
pub const LIGHT_ZONES: [(&str, u8); 4] = [("Front", 3), ("Scroll wheel", 2), ("Logo", 0), ("Back", 5)];

/// Lighting files: (file name, contents), the count file last. Zones with
/// the same light share one entry. Colors are corrected for LEDs here.
pub fn lighting_files(zones: &[(&str, crate::effects::OnboardLight)]) -> Vec<(String, Vec<u8>)> {
    use crate::effects::OnboardLight;
    let mut entries: Vec<(OnboardLight, Vec<u8>)> = Vec::new();
    for (name, number) in LIGHT_ZONES {
        let Some(&(_, light)) = zones.iter().find(|(z, _)| *z == name) else { continue };
        match entries.iter_mut().find(|(l, _)| *l == light) {
            Some((_, z)) => z.push(number),
            None => entries.push((light, vec![number])),
        }
    }
    let mut out = Vec::new();
    for (n, (light, zones)) in entries.iter().enumerate() {
        let d = match *light {
            OnboardLight::Steady(c) => {
                let c = c.to_led();
                vec![0x07, 0x00, 0x02, 0x00, 0x00, c.r, c.g, c.b, 0xff, 0, 0, 0, 0]
            }
            OnboardLight::Rainbow(_) => vec![0x03, 0x02, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            OnboardLight::Pulse(_, a, b) => {
                let (a, b) = (a.to_led(), b.to_led());
                vec![0x01, 0x02, 0x03, 0x00, 0x00, a.r, a.g, a.b, 0xff, b.r, b.g, b.b, 0xff]
            }
        };
        let mut k = (zones.len() as u32).to_le_bytes().to_vec();
        k.extend(zones);
        out.push((format!("lght_{n:02}.d"), d));
        out.push((format!("lght_{n:02}.k"), k));
    }
    out.push(("lghtcnt.cnt".into(), (entries.len() as u32).to_le_bytes().to_vec()));
    out
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
        assert_eq!(key_number(115), Some(0x82)); // Volume Up, as iCUE saved it
        assert_eq!(key_number(0x2f0), None);
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
        let buttons: Vec<u8> = p.map[4..].chunks(3).map(|e| e[0]).collect();
        assert_eq!(buttons, vec![7, 8, 9]);
    }

    #[test]
    fn skips_what_the_mouse_cant_hold() {
        let p = plan_map(&[
            bind(0x114, ActionDef::Combo { keys: vec![29, 0x110] }),
            bind(0x113, ActionDef::Key { key: 0x2f0 }),
            bind(0x2f2, ActionDef::Macro { steps: vec![MacroStep::Down { key: 30 }], mode: PlayMode::Repeat }),
            bind(0x110, ActionDef::Key { key: 30 }),
            BindingDef { device: InputDevice::Keyboard, input: 30, action: ActionDef::Key { key: 31 } },
        ]);
        assert_eq!(p.map, vec![0x41, 0x00, 0x00, 0x00]);
        assert_eq!(p.dat, vec![0x50, 0x00, 0x00, 0x00]);
        assert_eq!(p.skipped.iter().map(|s| s.0).collect::<Vec<_>>(), vec!["Forward", "Back", "Sniper"]);
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn mouse_buttons_like_icue() {
        // iCUE: Back = Middle, Forward = Back, DPI Up = Forward, DPI Down = Left.
        let p = plan_map(&[
            bind(0x113, ActionDef::Key { key: 0x112 }),
            bind(0x114, ActionDef::Key { key: 0x113 }),
            bind(0x2f0, ActionDef::Key { key: 0x114 }),
            bind(0x2f1, ActionDef::Key { key: 0x110 }),
        ]);
        assert_eq!(p.map, hex("4100040003ca8004cb8005cc8006c880"));
    }

    #[test]
    fn saves_like_icue() {
        // What iCUE wrote for: Back = Right click, Forward = Ctrl+C,
        // DPI Down = Volume Up (read back from the mouse, 2026-10-16).
        let p = plan_map(&[
            bind(0x113, ActionDef::Key { key: 0x111 }),
            bind(0x114, ActionDef::Combo { keys: vec![29, 46] }),
            bind(0x2f1, ActionDef::Key { key: 115 }),
        ]);
        assert_eq!(p.map, hex("4100020003c980068280"));
        assert_eq!(p.macros, vec![("M000".to_string(), hex("4d01000007000000203c80002034800000348000003c"))]);
        assert_eq!(p.dat, hex("50000100044d3030301600000001010000000000"));
        assert!(p.skipped.is_empty());
    }

    #[test]
    fn macro_matches_icue_recording() {
        // iCUE's M001: H for 94 ms, 2543 ms gap, I for 96 ms.
        let steps = [
            MacroStep::Down { key: 35 }, MacroStep::Wait { ms: 94 }, MacroStep::Up { key: 35 },
            MacroStep::Wait { ms: 2000 }, MacroStep::Wait { ms: 543 },
            MacroStep::Down { key: 23 }, MacroStep::Wait { ms: 96 }, MacroStep::Up { key: 23 },
        ];
        let (f, presses) = macro_file(&steps).unwrap();
        assert_eq!(f, hex("4d00000007000000202a805e002a89ef202080600020"));
        assert_eq!(presses, 2);
    }

    #[test]
    fn macros_never_leave_keys_held() {
        let (f, _) = macro_file(&[MacroStep::Down { key: 29 }, MacroStep::Down { key: 46 }]).unwrap();
        assert_eq!(f, hex("4d00000007000000203c80002034800000348000003c"));
        assert!(macro_file(&[MacroStep::Wait { ms: 5 }]).is_none());
        assert!(macro_file(&[MacroStep::Down { key: 0x110 }]).is_none());
        let (f, _) = macro_file(&[MacroStep::Down { key: 30 }, MacroStep::Wait { ms: 99_999 }, MacroStep::Up { key: 30 }]).unwrap();
        assert_eq!(&f[10..12], &[0xff, 0xff]);
    }

    #[test]
    fn lighting_like_icue() {
        use crate::effects::OnboardLight::{self, *};
        use crate::Rgb;
        // iCUE: Logo green, Front blue, Scroll wheel red.
        let f = lighting_files(&[("Front", Steady(Rgb::new(0, 0, 255))), ("Scroll wheel", Steady(Rgb::new(255, 0, 0))), ("Logo", Steady(Rgb::new(0, 255, 0)))]);
        assert!(f.contains(&("lght_02.d".into(), hex("070002000000ff00ff00000000"))));
        assert!(f.contains(&("lght_02.k".into(), hex("0100000000"))));
        assert!(f.contains(&("lght_00.d".into(), hex("07000200000000ffff00000000"))));
        assert!(f.contains(&("lght_00.k".into(), hex("0100000003"))));
        assert_eq!(f.last().unwrap(), &("lghtcnt.cnt".to_string(), hex("03000000")));
        // iCUE: rainbow on all four zones; pulse red/blue.
        let all = |l: OnboardLight| ["Front", "Scroll wheel", "Logo", "Back"].map(|z| (z, l)).to_vec();
        let f = lighting_files(&all(Rainbow(0)));
        assert_eq!(f, vec![
            ("lght_00.d".into(), hex("03020000000000000000000000")),
            ("lght_00.k".into(), hex("0400000003020005")),
            ("lghtcnt.cnt".into(), hex("01000000")),
        ]);
        let f = lighting_files(&all(Pulse(0, Rgb::new(255, 0, 0), Rgb::new(0, 0, 255))));
        assert_eq!(f[0], ("lght_00.d".into(), hex("0102030000ff0000ff0000ffff")));
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
