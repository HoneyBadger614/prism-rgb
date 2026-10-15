//! Installed apps, for "open an app" bindings. Reads the standard desktop
//! entry folders; nothing is run here.

use std::collections::BTreeMap;
use std::path::PathBuf;

use prism_core::bindings::valid_app_id;
use serde::Serialize;

#[derive(Serialize)]
pub struct App {
    pub id: String,
    pub name: String,
}

fn folders() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/usr/local/share/applications"),
        PathBuf::from("/var/lib/flatpak/exports/share/applications"),
    ];
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        dirs.push(home.join(".local/share/flatpak/exports/share/applications"));
        // last, so the user's own entries win
        dirs.push(home.join(".local/share/applications"));
    }
    dirs
}

/// Reads the name of a desktop entry, or None if it's hidden or not an app.
fn entry_name(text: &str) -> Option<String> {
    let mut name = None;
    let mut in_main = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
            continue;
        }
        if !in_main {
            continue;
        }
        match line.split_once('=') {
            Some(("Name", v)) if name.is_none() => name = Some(v.trim().to_string()),
            Some(("NoDisplay", "true")) | Some(("Hidden", "true")) => return None,
            Some(("Type", t)) if t.trim() != "Application" => return None,
            _ => {}
        }
    }
    name.filter(|n| !n.is_empty())
}

/// Every visible installed app, sorted by name.
pub fn list() -> Vec<App> {
    let mut found: BTreeMap<String, String> = BTreeMap::new();
    for dir in folders() {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            let Some(id) = path.file_name().and_then(|f| f.to_str()).and_then(|f| f.strip_suffix(".desktop")) else {
                continue;
            };
            if !valid_app_id(id) {
                continue;
            }
            match std::fs::read_to_string(&path).ok().as_deref().and_then(entry_name) {
                Some(name) => {
                    found.insert(id.to_string(), name);
                }
                None => {
                    found.remove(id); // a user entry can hide a system one
                }
            }
        }
    }
    let mut apps: Vec<App> = found.into_iter().map(|(id, name)| App { id, name }).collect();
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_names_and_skips_hidden() {
        assert_eq!(entry_name("[Desktop Entry]\nType=Application\nName=Firefox\n").as_deref(), Some("Firefox"));
        assert_eq!(entry_name("[Desktop Entry]\nName=X\nNoDisplay=true\n"), None);
        assert_eq!(entry_name("[Desktop Entry]\nName=Docs\nType=Link\n"), None);
        // names in other sections don't count
        assert_eq!(entry_name("[Desktop Entry]\nName=Main\n[Desktop Action new]\nName=New\n").as_deref(), Some("Main"));
    }
}
