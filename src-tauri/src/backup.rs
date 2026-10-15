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

/// Makes a new, empty backup folder.
pub fn new_folder(config_dir: &Path) -> std::io::Result<(String, PathBuf)> {
    let name = stamp();
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
