//! "Save to mouse": writes a Prism profile's button keys and DPI into one
//! of the Nightsword's onboard profiles, so they work without Prism.
//! A full backup is always taken first.

use hidapi::HidApi;
use prism_core::effects::Profile;
use prism_core::mouse_onboard::{self, MousePlan, Saved};
use prism_core::nightsword::Nightsword;
use serde::Serialize;

/// Profile folders the mouse can have.
const FOLDERS: std::ops::Range<u8> = 0..4;

#[derive(Debug, Clone, Serialize)]
pub struct OnboardProfile {
    pub folder: u8,
    pub name: String,
    /// What it holds now: sniper, then stages 1-3.
    pub sniper: u16,
    pub stages: [u16; 3],
}

#[derive(Debug, Clone, Serialize)]
pub struct SavePlan {
    pub profiles: Vec<OnboardProfile>,
    /// (button, key name) that will be saved.
    pub saved: Vec<(String, String)>,
    /// (button, why not)
    pub skipped: Vec<(String, String)>,
    /// The DPI that will be saved: Some only when the profile sets DPI.
    pub dpi: Option<(u16, [u16; 3])>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SaveReport {
    pub backup: String,
    pub profile: String,
    pub buttons: usize,
    pub dpi: bool,
    pub lighting: bool,
}

/// The onboard profiles on the mouse. Read-only.
pub fn profiles(m: &Nightsword, api: &HidApi) -> Vec<OnboardProfile> {
    FOLDERS
        .filter_map(|folder| {
            let f = m.read_file(api, folder, "PROFILE.I").ok()?;
            let (sniper, stages) = mouse_onboard::read_dpi(&f)?;
            let name = mouse_onboard::profile_name(&f).filter(|n| !n.trim().is_empty()).unwrap_or_else(|| format!("Profile {}", folder + 1));
            Some(OnboardProfile { folder, name, sniper, stages })
        })
        .collect()
}

/// What saving `profile` would write. Read-only.
pub fn plan(m: &Nightsword, api: &HidApi, profile: &Profile, key_name: impl Fn(u16) -> String) -> SavePlan {
    let p = mouse_onboard::plan_map(&profile.bindings);
    SavePlan {
        profiles: profiles(m, api),
        saved: p
            .saved
            .iter()
            .map(|(b, what)| {
                let text = match what {
                    Saved::Key(k) => key_name(*k),
                    Saved::Combo(keys) => keys.iter().map(|k| key_name(*k)).collect::<Vec<_>>().join(" + "),
                    Saved::Macro(n) => format!("a macro ({n} key press{})", if *n == 1 { "" } else { "es" }),
                };
                (b.to_string(), text)
            })
            .collect(),
        skipped: p.skipped.iter().map(|(b, why)| (b.to_string(), why.to_string())).collect(),
        dpi: profile.dpi.enabled.then(|| {
            let d = profile.dpi.clone().sanitized();
            (d.sniper, d.stages)
        }),
    }
}

/// Writes `profile` into onboard folder `folder`, then reads it back to
/// check. Call only after a backup.
/// `lighting`, if given, is each zone's light: (zone name, light).
pub fn save(m: &Nightsword, api: &HidApi, profile: &Profile, folder: u8, lighting: Option<&[(&str, prism_core::effects::OnboardLight)]>, backup: String) -> Result<SaveReport, String> {
    if !FOLDERS.contains(&folder) {
        return Err("That isn't one of the mouse's profiles".into());
    }
    let info = profile_i(m, api, folder)?;
    let name = mouse_onboard::profile_name(&info).unwrap_or_default();
    let MousePlan { map, dat, macros, saved, .. } = mouse_onboard::plan_map(&profile.bindings);

    let new_info = if profile.dpi.enabled {
        Some(mouse_onboard::patch_dpi(&info, &profile.dpi).ok_or("The mouse's profile file isn't in the expected format, so nothing was written")?)
    } else {
        None
    };

    // Macro files first, then the list that points to them, then the keys.
    // This replaces the profile's whole button setup, including any macros
    // iCUE left (a macro would win over a key on the same button).
    for (file, data) in &macros {
        write_checked(m, api, folder, file, data)?;
    }
    write_checked(m, api, folder, "PROFILE.DAT", &dat)?;
    write_checked(m, api, folder, "PROFILE.MAP", &map)?;
    if let Some(f) = &new_info {
        write_checked(m, api, folder, "PROFILE.I", f)?;
    }
    if let Some(zones) = lighting {
        // The count file comes last, so the entries are all there first.
        for (file, data) in mouse_onboard::lighting_files(zones) {
            write_checked(m, api, folder, &file, &data)?;
        }
    }
    Ok(SaveReport { backup, profile: name, buttons: saved.len(), dpi: new_info.is_some(), lighting: lighting.is_some() })
}

fn profile_i(m: &Nightsword, api: &HidApi, folder: u8) -> Result<Vec<u8>, String> {
    let f = m.read_file(api, folder, "PROFILE.I").map_err(|e| format!("Couldn't read the mouse's profile: {e}"))?;
    if mouse_onboard::read_dpi(&f).is_none() {
        return Err("The mouse's profile file isn't in the expected format, so nothing was written".into());
    }
    Ok(f)
}

fn write_checked(m: &Nightsword, api: &HidApi, folder: u8, name: &str, data: &[u8]) -> Result<(), String> {
    m.write_named(api, folder, name, data).map_err(|e| format!("Writing {name} failed: {e}"))?;
    match m.read_file(api, folder, name) {
        Ok(back) if back == data => Ok(()),
        Ok(_) => Err(format!("{name} read back different from what was written. Restore the backup from the list to undo.")),
        Err(e) => Err(format!("Couldn't read {name} back to check it: {e}")),
    }
}
