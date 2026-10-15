//! "Start at login": a desktop entry in ~/.config/autostart that starts
//! Prism quietly in the tray.

use std::path::PathBuf;

fn entry() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(config.join("autostart").join("prism-rgb.desktop"))
}

pub fn is_on() -> bool {
    entry().is_some_and(|p| p.exists())
}

pub fn set(on: bool) -> std::io::Result<()> {
    let path = entry().ok_or_else(|| std::io::Error::other("no home folder"))?;
    if !on {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    let exe = std::env::current_exe()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(
        &path,
        format!(
            "[Desktop Entry]\nType=Application\nName=Prism\nComment=Corsair lighting and key bindings\n\
             Exec=\"{}\" --hidden\nIcon=prism-rgb\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
            exe.display()
        ),
    )
}
