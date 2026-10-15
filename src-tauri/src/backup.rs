//! Onboard memory backups: copies of every file stored on the keyboard and
//! mouse, saved in Prism's config folder. Read-only towards the devices.
//!
//! Layout: <config>/backups/<YYYY-MM-DD_HH-MM-SS>/
//!   k65/<file number>.bin            (e.g. 6d68.bin)
//!   nightsword/<folder>_<name>       (e.g. 1_PROFILE.MAP)
//!   manifest.json

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeviceBackup {
    /// Whether the device was connected and read.
    pub read: bool,
    /// (file, size in bytes)
    pub files: Vec<(String, usize)>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// Folder name, which is also when it was taken.
    pub name: String,
    pub created: String,
    /// Why it was made, if not by pressing Back up now.
    #[serde(default)]
    pub note: Option<String>,
    pub keyboard: DeviceBackup,
    pub mouse: DeviceBackup,
}

/// Where backups go.
pub fn root(config_dir: &Path) -> PathBuf {
    config_dir.join("backups")
}

/// Local time as "YYYY-MM-DD_HH-MM-SS", via the system `date` (no time zone
/// library needed for one timestamp).
fn stamp() -> String {
    std::process::Command::new("date")
        .arg("+%Y-%m-%d_%H-%M-%S")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            format!("backup-{secs}")
        })
}

/// Makes a new, empty backup folder. `suffix` marks special ones.
pub fn new_folder(config_dir: &Path, suffix: &str) -> std::io::Result<(String, PathBuf)> {
    let name = format!("{}{suffix}", stamp());
    let dir = root(config_dir).join(&name);
    std::fs::create_dir_all(&dir)?;
    Ok((name, dir))
}

/// Saves one device's files into `dir/<device>/`.
pub fn save_files(dir: &Path, device: &str, files: &[(String, Vec<u8>)]) -> std::io::Result<Vec<(String, usize)>> {
    let d = dir.join(device);
    std::fs::create_dir_all(&d)?;
    let mut out = Vec::new();
    for (name, data) in files {
        // names come from the device; keep them to safe file names
        let safe: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '_' }).collect();
        std::fs::write(d.join(&safe), data)?;
        out.push((safe, data.len()));
    }
    Ok(out)
}

pub fn write_manifest(dir: &Path, m: &Manifest) -> std::io::Result<()> {
    std::fs::write(dir.join("manifest.json"), serde_json::to_string_pretty(m).unwrap_or_default())
}

/// Every backup, newest first.
pub fn list(config_dir: &Path) -> Vec<Manifest> {
    let Ok(entries) = std::fs::read_dir(root(config_dir)) else { return vec![] };
    let mut out: Vec<Manifest> = entries
        .flatten()
        .filter_map(|e| std::fs::read_to_string(e.path().join("manifest.json")).ok())
        .filter_map(|s| serde_json::from_str(&s).ok())
        .collect();
    out.sort_by(|a, b| b.name.cmp(&a.name));
    out
}

/* ---------- taking a backup ---------- */

use hidapi::HidApi;
use prism_core::k65_mini::K65Mini;
use prism_core::nightsword::Nightsword;

/// Reads both devices (whichever are connected) into `dir`. Read-only.
pub fn take(kb: Option<&K65Mini>, mouse: Option<&Nightsword>, api: Option<&HidApi>, dir: &Path, name: String, note: Option<String>) -> Manifest {
    let mut keyboard = DeviceBackup::default();
    if let Some(k) = kb {
        keyboard.read = true;
        let mut ok = Vec::new();
        for (id, r) in k.read_all_files() {
            match r {
                Ok(data) => ok.push((format!("{id:04x}.bin"), data)),
                Err(e) => keyboard.error = Some(format!("file {id:#06x}: {e}")),
            }
        }
        match save_files(dir, "k65", &ok) {
            Ok(files) => keyboard.files = files,
            Err(e) => keyboard.error = Some(e.to_string()),
        }
    }
    let mut mouse_b = DeviceBackup::default();
    if let (Some(m), Some(api)) = (mouse, api) {
        mouse_b.read = true;
        match m.read_all_files(api) {
            Ok(files) => {
                let named: Vec<(String, Vec<u8>)> = files.into_iter().map(|(f, n, d)| (format!("{f}_{n}"), d)).collect();
                match save_files(dir, "nightsword", &named) {
                    Ok(saved) => mouse_b.files = saved,
                    Err(e) => mouse_b.error = Some(e.to_string()),
                }
            }
            Err(e) => mouse_b.error = Some(e.to_string()),
        }
    }
    // "2026-10-15_23-40-00" -> "2026-10-15 23:40:00"
    let created = match name.split_once('_') {
        Some((date, time)) => format!("{date} {}", time.split('_').next().unwrap_or(time).replace('-', ":")),
        None => name.clone(),
    };
    let m = Manifest { name, created, note, keyboard, mouse: mouse_b };
    let _ = write_manifest(dir, &m);
    m
}

/* ---------- restoring ---------- */

/// The files in a backup folder.
struct Loaded {
    k65: Vec<(u16, Vec<u8>)>,
    mouse: Vec<(u8, String, Vec<u8>)>,
}

fn load(dir: &Path) -> std::io::Result<Loaded> {
    let mut k65 = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir.join("k65")) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(id) = name.strip_suffix(".bin").and_then(|h| u16::from_str_radix(h, 16).ok()) {
                k65.push((id, std::fs::read(e.path())?));
            }
        }
    }
    let mut mouse = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir.join("nightsword")) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some((folder, file)) = name.split_once('_').and_then(|(f, n)| Some((f.parse::<u8>().ok()?, n.to_string()))) {
                if file != "profiles.list" {
                    mouse.push((folder, file, std::fs::read(e.path())?));
                }
            }
        }
    }
    k65.sort_by_key(|(id, _)| *id);
    mouse.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    Ok(Loaded { k65, mouse })
}

/// What a restore would do on one device.
#[derive(Debug, Clone, Default, Serialize)]
pub struct DevicePlan {
    pub connected: bool,
    /// Files that would be written (they differ or are missing on the device).
    pub write: Vec<String>,
    /// Files already identical (left alone).
    pub same: usize,
    /// Files on the device that aren't in the backup and would be deleted.
    pub delete: Vec<String>,
    /// Extra files Prism can't remove yet (the mouse's delete isn't known).
    pub cannot_remove: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Plan {
    pub keyboard: DevicePlan,
    pub mouse: DevicePlan,
}

/// The order K65 files are written in: profile data first, then the files
/// that point to it, so the keyboard never points at a missing file.
fn k65_order(id: u16) -> (u8, u16) {
    match id {
        0x000f => (1, id), // profile list
        0x0025 => (2, id), // points to the profile list
        0x0002 => (3, id),
        _ => (0, id),
    }
}

/// The order mouse files are written in, inside each folder.
fn mouse_order(name: &str) -> u8 {
    match name {
        n if n.starts_with("lght_") => 0,
        "lghtcnt.cnt" => 1,
        "PROFILE.MAP" => 2,
        "PROFILE.I" => 3,
        "M000" => 4,
        "PROFILE.DAT" => 5,
        _ => 6,
    }
}

/// Compares a backup with what's on the devices now. Read-only.
pub fn plan(kb: Option<&K65Mini>, mouse: Option<&Nightsword>, api: Option<&HidApi>, dir: &Path) -> Plan {
    let mut p = Plan::default();
    let loaded = match load(dir) {
        Ok(l) => l,
        Err(e) => {
            p.keyboard.error = Some(e.to_string());
            return p;
        }
    };
    if let (Some(k), false) = (kb, loaded.k65.is_empty()) {
        p.keyboard.connected = true;
        let now: std::collections::BTreeMap<u16, Vec<u8>> =
            k.read_all_files().into_iter().filter_map(|(id, r)| r.ok().map(|d| (id, d))).collect();
        for (id, data) in &loaded.k65 {
            if now.get(id) == Some(data) {
                p.keyboard.same += 1;
            } else {
                p.keyboard.write.push(format!("{id:04x}"));
            }
        }
        for id in now.keys() {
            if !loaded.k65.iter().any(|(b, _)| b == id) && !K65Mini::is_system_file(*id) {
                p.keyboard.delete.push(format!("{id:04x}"));
            }
        }
    }
    if let (Some(m), Some(api), false) = (mouse, api, loaded.mouse.is_empty()) {
        p.mouse.connected = true;
        match m.read_all_files(api) {
            Ok(now) => {
                for (folder, name, data) in &loaded.mouse {
                    let cur = now.iter().find(|(f, n, _)| f == folder && n == name).map(|(_, _, d)| d);
                    if cur == Some(data) {
                        p.mouse.same += 1;
                    } else {
                        p.mouse.write.push(format!("{folder}/{name}"));
                    }
                }
                for (folder, name, _) in &now {
                    if name != "profiles.list" && !loaded.mouse.iter().any(|(f, n, _)| f == folder && n == name) {
                        p.mouse.cannot_remove.push(format!("{folder}/{name}"));
                    }
                }
            }
            Err(e) => p.mouse.error = Some(e.to_string()),
        }
    }
    p
}

/// What a restore did.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Restored {
    pub written: Vec<String>,
    pub deleted: Vec<String>,
    pub failed: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RestoreReport {
    pub keyboard: Restored,
    pub mouse: Restored,
    /// The backup made automatically just before restoring.
    pub safety_backup: Option<String>,
}

/// Writes a backup back onto the devices: only the files that differ, in a
/// safe order. CHANGES THE DEVICES: the caller makes a fresh backup first.
pub fn restore(kb: Option<&K65Mini>, mouse: Option<&Nightsword>, api: Option<&HidApi>, dir: &Path, plan: &Plan) -> RestoreReport {
    let mut r = RestoreReport::default();
    let Ok(loaded) = load(dir) else { return r };
    if let Some(k) = kb {
        let mut todo: Vec<&(u16, Vec<u8>)> =
            loaded.k65.iter().filter(|(id, _)| plan.keyboard.write.contains(&format!("{id:04x}"))).collect();
        todo.sort_by_key(|(id, _)| k65_order(*id));
        for (id, data) in todo {
            match k.write_file(*id, data) {
                Ok(()) => r.keyboard.written.push(format!("{id:04x}")),
                Err(e) => r.keyboard.failed.push(format!("{id:04x}: {e}")),
            }
        }
        // Remove leftovers last, once nothing points to them any more.
        for name in &plan.keyboard.delete {
            if let Ok(id) = u16::from_str_radix(name, 16) {
                match k.delete_file(id) {
                    Ok(()) => r.keyboard.deleted.push(name.clone()),
                    Err(e) => r.keyboard.failed.push(format!("{name}: {e}")),
                }
            }
        }
    }
    if let (Some(m), Some(api)) = (mouse, api) {
        let mut todo: Vec<&(u8, String, Vec<u8>)> =
            loaded.mouse.iter().filter(|(f, n, _)| plan.mouse.write.contains(&format!("{f}/{n}"))).collect();
        todo.sort_by_key(|(f, n, _)| (*f, mouse_order(n)));
        for (folder, name, data) in todo {
            match m.write_named(api, *folder, name, data) {
                Ok(()) => r.mouse.written.push(format!("{folder}/{name}")),
                Err(e) => r.mouse.failed.push(format!("{folder}/{name}: {e}")),
            }
        }
    }
    r
}

/// Whether `name` is a plain backup folder name (no path tricks).
pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.chars().all(|c| c.is_ascii_alphanumeric() || "-_".contains(c))
}
