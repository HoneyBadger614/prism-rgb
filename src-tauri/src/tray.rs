//! The system tray icon and its menu, and the main window's lifecycle.
//!
//! Closing the window doesn't quit Prism: lighting, bindings and DPI keep
//! running from the tray. The window (and the web engine behind it, which is
//! most of Prism's memory) is destroyed on close and rebuilt when reopened.

use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, Wry};

use prism_core::effects::LightingStore;

use crate::{autostart, AppState};

const TRAY_ID: &str = "prism";

/// Set when quitting from the tray, so closing doesn't trigger a slim-down.
static QUITTING: AtomicBool = AtomicBool::new(false);

/// After the window closes, the web engine's helper processes and memory
/// stay around (~180 MB). To really get small in the background, Prism
/// saves its settings and restarts itself as a fresh tray-only process
/// (~40 MB). Devices aren't handed back first, so lighting carries on;
/// the new process picks them straight up again.
pub fn window_closed(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        // give a quick reopen (or a quit) the chance to happen first
        std::thread::sleep(Duration::from_millis(1500));
        if QUITTING.load(Ordering::SeqCst) || !app.webview_windows().is_empty() {
            return;
        }
        let engine = &app.state::<AppState>().engine;
        engine.flush();
        // Tell the new process how far along the effects are, so they carry on.
        let resume = format!("--resume-at={:.3}", engine.elapsed());
        if let Ok(exe) = std::env::current_exe() {
            use std::os::unix::process::CommandExt;
            let err = std::process::Command::new(exe).arg("--hidden").arg(resume).exec();
            eprintln!("Prism: couldn't restart in the background: {err}");
        }
    });
}

/// Shows the main window, creating it if it was closed.
pub fn show_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        return;
    }
    let built = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("Prism")
        .inner_size(1180.0, 760.0)
        .min_inner_size(820.0, 560.0)
        .build();
    if let Err(e) = built {
        eprintln!("Prism: couldn't open the window: {e}");
    }
}

/// Makes `store` the live settings: lighting, bindings, the tray menu and
/// the open window (if any) all follow.
pub fn apply_store(app: &AppHandle, store: LightingStore) {
    let state = app.state::<AppState>();
    state.engine.set_config(store.clone());
    state.remap.apply(&store, &state.engine);
    let _ = app.emit("store-updated", &store);
    refresh(app);
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let store = app.state::<AppState>().engine.config();

    let open = MenuItem::with_id(app, "open", "Open Prism", true, None::<&str>)?;
    let profiles: Vec<CheckMenuItem<Wry>> = store
        .profiles
        .iter()
        .enumerate()
        .map(|(i, p)| CheckMenuItem::with_id(app, format!("profile:{i}"), &p.name, true, i == store.active, None::<&str>))
        .collect::<tauri::Result<_>>()?;
    let profile_refs: Vec<&dyn IsMenuItem<Wry>> = profiles.iter().map(|p| p as &dyn IsMenuItem<Wry>).collect();
    let profile_menu = Submenu::with_id_and_items(app, "profiles", "Profile", true, &profile_refs)?;
    let bindings = CheckMenuItem::with_id(app, "bindings", "Key bindings on", true, store.bindings_on, None::<&str>)?;
    let login = CheckMenuItem::with_id(app, "login", "Start at login", true, autostart::is_on(), None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Prism", true, None::<&str>)?;

    Menu::with_items(
        app,
        &[
            &open,
            &PredefinedMenuItem::separator(app)?,
            &profile_menu,
            &bindings,
            &login,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )
}

/// Rebuilds the menu, e.g. after profiles change.
pub fn refresh(app: &AppHandle) {
    if let (Some(tray), Ok(menu)) = (app.tray_by_id(TRAY_ID), build_menu(app)) {
        let _ = tray.set_menu(Some(menu));
    }
}

fn on_menu(app: &AppHandle, event: MenuEvent) {
    let id = event.id.as_ref();
    let mut store = app.state::<AppState>().engine.config();
    match id {
        "open" => show_window(app),
        "quit" => {
            QUITTING.store(true, Ordering::SeqCst);
            app.exit(0);
        }
        "bindings" => {
            store.bindings_on = !store.bindings_on;
            apply_store(app, store);
        }
        "login" => {
            if let Err(e) = autostart::set(!autostart::is_on()) {
                eprintln!("Prism: couldn't change start at login: {e}");
            }
            refresh(app);
        }
        _ => {
            if let Some(i) = id.strip_prefix("profile:").and_then(|n| n.parse::<usize>().ok()) {
                if i < store.profiles.len() {
                    store.active = i;
                    apply_store(app, store);
                }
            }
        }
    }
}

/// Puts Prism's icon in the system tray.
pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Prism")
        .menu(&build_menu(app)?)
        .show_menu_on_left_click(true)
        .on_menu_event(on_menu);
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}
