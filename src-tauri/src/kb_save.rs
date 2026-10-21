//! "Save to keyboard": writes a Prism profile's key bindings into one of the
//! K65's onboard profiles, so they work without Prism. A full backup is
//! always taken first.

use prism_core::effects::Profile;
use prism_core::effects::{OnboardLight, OwnEffect};
use prism_core::k65_mini::{K65Mini, Key};
use prism_core::kb_onboard::{self, Saved};
use serde::Serialize;

/// The keyboard's list of onboard profiles.
const PROFILE_LIST: u16 = 0x000f;

#[derive(Debug, Clone, Serialize)]
pub struct OnboardProfile {
    /// The profile's root file.
    pub root: u16,
    pub name: String,
    /// How many keys it has bound now.
    pub bound: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SavePlan {
    pub profiles: Vec<OnboardProfile>,
    pub saved: Vec<(String, String)>,
    pub skipped: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SaveReport {
    pub backup: String,
    pub profile: String,
    pub keys: usize,
    /// How many lighting layers were saved, if lighting was.
    pub layers: Option<usize>,
}

struct Found {
    root: u16,
    info: kb_onboard::Root,
    actions: Vec<u16>,
    layers: Vec<u16>,
}

fn find_profiles(kb: &K65Mini) -> Result<Vec<Found>, String> {
    let list = kb.read_file(PROFILE_LIST).map_err(|e| format!("Couldn't read the keyboard's profile list: {e}"))?;
    let mut out = Vec::new();
    for root in kb_onboard::profile_roots(&list) {
        let Some(info) = kb.read_file(root).ok().and_then(|f| kb_onboard::parse_root(&f)) else { continue };
        let Some(actions) = kb.read_file(info.key_list).ok().and_then(|f| kb_onboard::key_list_actions(&f)) else { continue };
        let layers = kb.read_file(info.layers).ok().and_then(|f| kb_onboard::layer_list(&f)).unwrap_or_default();
        out.push(Found { root, info, actions, layers });
    }
    Ok(out)
}

/// What saving `profile` would write. Read-only.
pub fn plan(kb: &K65Mini, keys: &[Key], profile: &Profile, key_name: impl Fn(u16) -> String) -> Result<SavePlan, String> {
    let p = kb_onboard::plan(&profile.bindings, keys);
    Ok(SavePlan {
        profiles: find_profiles(kb)?
            .into_iter()
            .map(|f| OnboardProfile { root: f.root, name: f.info.name, bound: f.actions.len() })
            .collect(),
        saved: p
            .saved
            .iter()
            .map(|(k, what)| {
                let text = match what {
                    Saved::Key(c) => key_name(*c),
                    Saved::Combo(cs) => cs.iter().map(|c| key_name(*c)).collect::<Vec<_>>().join(" + "),
                    Saved::Macro { taps, shortened } => format!(
                        "a macro ({taps} key tap{}){}",
                        if *taps == 1 { "" } else { "s" },
                        if *shortened { "; pauses longer than a quarter second are shortened, the keyboard can't store longer ones" } else { "" }
                    ),
                };
                (k.to_string(), text)
            })
            .collect(),
        skipped: p.skipped.iter().map(|(k, why)| (k.to_string(), why.to_string())).collect(),
    })
}

/// Writes `profile`'s bindings (and, if given, the lighting: (slot, light)
/// for every key, and the typing effects with their slots) into the
/// onboard profile whose root file is `root`,
/// reading every file back to check it. Call only after a backup;
/// `existing` is every file number the backup found.
pub fn save(kb: &K65Mini, keys: &[Key], profile: &Profile, root: u16, existing: &[u16], lighting: Option<(&[(u8, OnboardLight)], &[(OwnEffect, Vec<u8>)])>, backup: String) -> Result<SaveReport, String> {
    let all = find_profiles(kb)?;
    let target = all.iter().find(|f| f.root == root).ok_or("That isn't one of the keyboard's profiles")?;
    let old_flags = kb.read_file(target.info.flags).map_err(|e| format!("Couldn't read the keyboard's key flags: {e}"))?;
    let p = kb_onboard::plan(&profile.bindings, keys);
    let bound: Vec<u8> = p.actions.iter().map(|a| a.0).collect();
    let new_flags = kb_onboard::flags(&old_flags, &bound).ok_or("The keyboard's key flags aren't in the expected format, so nothing was written")?;

    // Files the other profiles use are never touched.
    let others: Vec<u16> = all.iter().filter(|f| f.root != root).flat_map(|f| f.actions.iter().chain(&f.layers).copied()).collect();
    let reusable: Vec<u16> = target.actions.iter().copied().filter(|id| !others.contains(id)).collect();
    let old_list = kb.read_file(target.info.layers).ok();
    if lighting.is_some() && old_list.as_deref().and_then(kb_onboard::layer_list).is_none() {
        return Err("The keyboard's lighting list isn't in the expected format, so nothing was written".into());
    }

    // File numbers for the actions: this profile's old ones first, then new
    // ones after the highest number in use.
    let mut next = existing.iter().copied().filter(|id| id >> 8 == 0x6d).max().unwrap_or(0x6d70) + 1;
    let mut ids = Vec::new();
    for i in 0..p.actions.len() {
        match reusable.get(i) {
            Some(&id) => ids.push(id),
            None => {
                if next >> 8 != 0x6d {
                    return Err("The keyboard has no free file numbers left".into());
                }
                ids.push(next);
                next += 1;
            }
        }
    }

    for ((_, data, _), &id) in p.actions.iter().zip(&ids) {
        write_checked(kb, id, data)?;
    }
    let list: Vec<(u8, u16, u8)> = p.actions.iter().zip(&ids).map(|((slot, _, kind), &id)| (*slot, id, *kind)).collect();
    write_checked(kb, target.info.key_list, &kb_onboard::key_list(&list))?;
    write_checked(kb, target.info.flags, &new_flags)?;

    // Old action files nothing points to any more.
    for id in reusable.iter().skip(ids.len()) {
        let _ = kb.delete_file(*id);
    }

    // Lighting: typing effects, the keyboard's own effects, color loops and
    // steady colors as layers, then the list pointing to them.
    let mut layers = None;
    if let Some((lights, typing)) = lighting {
        let files = kb_onboard::light_layers(lights, typing);
        let reuse: Vec<u16> = target.layers.iter().copied().filter(|id| !others.contains(id)).collect();
        let mut layer_ids = Vec::new();
        for (i, file) in files.iter().enumerate() {
            let id = match reuse.get(i) {
                Some(&id) => id,
                None => {
                    if next >> 8 != 0x6d {
                        return Err("The keyboard has no free file numbers left".into());
                    }
                    next += 1;
                    next - 1
                }
            };
            write_checked(kb, id, file)?;
            layer_ids.push(id);
        }
        write_checked(kb, target.info.layers, &kb_onboard::layer_list_file(&layer_ids))?;
        for id in reuse.iter().skip(layer_ids.len()) {
            let _ = kb.delete_file(*id);
        }
        layers = Some(layer_ids.len());
    }
    Ok(SaveReport { backup, profile: target.info.name.clone(), keys: p.saved.len(), layers })
}

fn write_checked(kb: &K65Mini, id: u16, data: &[u8]) -> Result<(), String> {
    kb.write_file(id, data).map_err(|e| format!("Writing file {id:#06x} failed: {e}"))?;
    match kb.read_file(id) {
        Ok(back) if back == data => Ok(()),
        Ok(_) => Err(format!("File {id:#06x} read back different from what was written. Restore the backup from the list to undo.")),
        Err(e) => Err(format!("Couldn't read file {id:#06x} back to check it: {e}")),
    }
}
