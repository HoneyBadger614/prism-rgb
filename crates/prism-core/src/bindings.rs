//! Key and button bindings, as saved in profiles.
//!
//! Keys are Linux key codes (KEY_A = 30, BTN_SIDE = 0x113, ...). This is
//! plain data; the input layer (`prism-input`) does the actual remapping.

use serde::{Deserialize, Serialize};

pub const MAX_BINDINGS: usize = 200;
pub const MAX_COMBO_KEYS: usize = 6;
/// Highest key code Linux uses.
pub const MAX_CODE: u16 = 0x2ff;
/// The left mouse button can never be bound, so clicking always works.
pub const BTN_LEFT: u16 = 0x110;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InputDevice {
    Keyboard,
    Mouse,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ActionDef {
    /// Acts as another key (including media keys).
    Key { key: u16 },
    /// Holds several keys together, e.g. Ctrl+Shift+S.
    Combo { keys: Vec<u16> },
    /// Opens an installed app, by its desktop id (e.g. "firefox").
    Launch { app: String },
    /// Changes the mouse's DPI.
    Dpi { op: DpiOp },
    /// Does nothing.
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DpiOp {
    /// Next stage up (stops at the top).
    Up,
    /// Next stage down (stops at the bottom).
    Down,
    /// Next stage, back to the first after the last.
    Cycle,
    /// Sniper DPI while held.
    Sniper,
}

/// DPI settings for one profile. Prism only touches the mouse's DPI when
/// `enabled` is on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DpiSettings {
    pub enabled: bool,
    pub stages: [u16; 3],
    pub sniper: u16,
    /// Stage to start on, 1-3.
    pub start: u8,
}

impl Default for DpiSettings {
    fn default() -> Self {
        Self { enabled: false, stages: [800, 1600, 3200], sniper: 400, start: 2 }
    }
}

impl DpiSettings {
    pub fn sanitized(mut self) -> Self {
        self.stages = self.stages.map(crate::nightsword::clamp_dpi);
        self.sniper = crate::nightsword::clamp_dpi(self.sniper);
        self.start = self.start.clamp(1, 3);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindingDef {
    pub device: InputDevice,
    /// The key or button that's bound.
    pub input: u16,
    pub action: ActionDef,
}

/// Whether `id` looks like a desktop app id: letters, digits, `.`, `_`, `-`,
/// nothing that could reach a shell or a path.
pub fn valid_app_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

impl ActionDef {
    fn sanitized(self) -> Option<Self> {
        let ok = |k: &u16| *k > 0 && *k <= MAX_CODE;
        Some(match self {
            ActionDef::Key { key } => ok(&key).then_some(ActionDef::Key { key })?,
            ActionDef::Combo { mut keys } => {
                keys.retain(ok);
                keys.dedup();
                keys.truncate(MAX_COMBO_KEYS);
                if keys.is_empty() {
                    return None;
                }
                ActionDef::Combo { keys }
            }
            ActionDef::Launch { app } => valid_app_id(&app).then_some(ActionDef::Launch { app })?,
            ActionDef::Dpi { op } => ActionDef::Dpi { op },
            ActionDef::Disabled => ActionDef::Disabled,
        })
    }
}

/// Drops anything invalid: bad codes, bad app ids, the left mouse button,
/// and duplicates (the last binding for a key wins).
pub fn sanitize(bindings: Vec<BindingDef>) -> Vec<BindingDef> {
    let mut out: Vec<BindingDef> = Vec::new();
    for b in bindings {
        if b.input == 0 || b.input > MAX_CODE || (b.device == InputDevice::Mouse && b.input == BTN_LEFT) {
            continue;
        }
        let Some(action) = b.action.sanitized() else { continue };
        out.retain(|o| !(o.device == b.device && o.input == b.input));
        out.push(BindingDef { action, ..b });
    }
    out.truncate(MAX_BINDINGS);
    out
}

/// Keys offered in the "act as" list, grouped, with friendly names.
pub const CATALOG: &[(&str, &[(u16, &str)])] = &[
    ("Letters", &[
        (30, "A"), (48, "B"), (46, "C"), (32, "D"), (18, "E"), (33, "F"), (34, "G"), (35, "H"), (23, "I"),
        (36, "J"), (37, "K"), (38, "L"), (50, "M"), (49, "N"), (24, "O"), (25, "P"), (16, "Q"), (19, "R"),
        (31, "S"), (20, "T"), (22, "U"), (47, "V"), (17, "W"), (45, "X"), (21, "Y"), (44, "Z"),
    ]),
    ("Numbers", &[
        (2, "1"), (3, "2"), (4, "3"), (5, "4"), (6, "5"), (7, "6"), (8, "7"), (9, "8"), (10, "9"), (11, "0"),
    ]),
    ("Function", &[
        (59, "F1"), (60, "F2"), (61, "F3"), (62, "F4"), (63, "F5"), (64, "F6"), (65, "F7"), (66, "F8"),
        (67, "F9"), (68, "F10"), (87, "F11"), (88, "F12"), (183, "F13"), (184, "F14"), (185, "F15"),
        (186, "F16"), (187, "F17"), (188, "F18"), (189, "F19"), (190, "F20"), (191, "F21"), (192, "F22"),
        (193, "F23"), (194, "F24"),
    ]),
    ("Modifiers", &[
        (29, "Left Ctrl"), (42, "Left Shift"), (56, "Left Alt"), (125, "Super"), (97, "Right Ctrl"),
        (54, "Right Shift"), (100, "Right Alt"),
    ]),
    ("Editing", &[
        (1, "Esc"), (15, "Tab"), (58, "Caps Lock"), (14, "Backspace"), (28, "Enter"), (57, "Space"),
        (111, "Delete"), (110, "Insert"), (99, "Print Screen"), (70, "Scroll Lock"), (119, "Pause"),
        (127, "Menu"),
    ]),
    ("Navigation", &[
        (103, "Up"), (108, "Down"), (105, "Left"), (106, "Right"), (102, "Home"), (107, "End"),
        (104, "Page Up"), (109, "Page Down"),
    ]),
    ("Symbols", &[
        (12, "-"), (13, "="), (26, "["), (27, "]"), (43, "\\"), (39, ";"), (40, "'"), (41, "`"),
        (51, ","), (52, "."), (53, "/"),
    ]),
    ("Keypad", &[
        (82, "Keypad 0"), (79, "Keypad 1"), (80, "Keypad 2"), (81, "Keypad 3"), (75, "Keypad 4"),
        (76, "Keypad 5"), (77, "Keypad 6"), (71, "Keypad 7"), (72, "Keypad 8"), (73, "Keypad 9"),
        (98, "Keypad /"), (55, "Keypad *"), (74, "Keypad -"), (78, "Keypad +"), (96, "Keypad Enter"),
        (83, "Keypad ."),
    ]),
    ("Media", &[
        (164, "Play / Pause"), (163, "Next track"), (165, "Previous track"), (166, "Stop"),
        (115, "Volume up"), (114, "Volume down"), (113, "Mute"),
    ]),
    ("Mouse", &[
        (0x111, "Right click"), (0x112, "Middle click"), (0x113, "Back"), (0x114, "Forward"),
    ]),
];

/// A friendly name for a key code, if Prism has one.
pub fn friendly_name(code: u16) -> Option<&'static str> {
    CATALOG.iter().flat_map(|(_, keys)| keys.iter()).find(|(c, _)| *c == code).map(|(_, n)| *n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bind(device: InputDevice, input: u16, action: ActionDef) -> BindingDef {
        BindingDef { device, input, action }
    }

    #[test]
    fn sanitize_drops_bad_and_dedupes() {
        use ActionDef::*;
        let out = sanitize(vec![
            bind(InputDevice::Keyboard, 16, Key { key: 44 }),
            bind(InputDevice::Keyboard, 16, Key { key: 45 }), // replaces the one above
            bind(InputDevice::Mouse, BTN_LEFT, Disabled),     // never allowed
            bind(InputDevice::Keyboard, 0x400, Disabled),     // not a real key
            bind(InputDevice::Keyboard, 30, Key { key: 0 }),  // empty action
            bind(InputDevice::Keyboard, 31, Combo { keys: vec![29, 29, 0, 42, 1, 2, 3, 4, 5] }),
            bind(InputDevice::Keyboard, 32, Launch { app: "firefox; rm -rf ~".into() }),
            bind(InputDevice::Mouse, 0x113, Launch { app: "org.kde.dolphin".into() }),
        ]);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0], bind(InputDevice::Keyboard, 16, Key { key: 45 }));
        assert_eq!(out[1].action, Combo { keys: vec![29, 42, 1, 2, 3, 4] });
        assert_eq!(out[2].action, Launch { app: "org.kde.dolphin".into() });
    }

    #[test]
    fn app_ids() {
        assert!(valid_app_id("firefox"));
        assert!(valid_app_id("org.kde.konsole"));
        assert!(!valid_app_id(""));
        assert!(!valid_app_id("../evil"));
        assert!(!valid_app_id("a b"));
        assert!(!valid_app_id("x;y"));
    }

    #[test]
    fn json_shape() {
        let b = bind(InputDevice::Mouse, 0x113, ActionDef::Combo { keys: vec![29, 31] });
        let j = serde_json::to_string(&b).unwrap();
        assert_eq!(j, r#"{"device":"mouse","input":275,"action":{"kind":"combo","keys":[29,31]}}"#);
        assert_eq!(serde_json::from_str::<BindingDef>(&j).unwrap(), b);
    }

    #[test]
    fn dpi_settings_clamp() {
        let d = DpiSettings { enabled: true, stages: [10, 1630, 40000], sniper: 0, start: 7 }.sanitized();
        assert_eq!((d.stages, d.sniper, d.start), ([100, 1650, 18000], 100, 3));
        let j = r#"{"device":"mouse","input":754,"action":{"kind":"dpi","op":"sniper"}}"#;
        assert_eq!(serde_json::from_str::<BindingDef>(j).unwrap().action, ActionDef::Dpi { op: DpiOp::Sniper });
    }

    #[test]
    fn names() {
        assert_eq!(friendly_name(164), Some("Play / Pause"));
        assert_eq!(friendly_name(98), Some("Keypad /"));
        assert_eq!(friendly_name(9999), None);
    }
}
