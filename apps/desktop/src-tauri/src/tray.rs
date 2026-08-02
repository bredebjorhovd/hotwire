//! Menu-bar popover foundation.
//!
//! A tray icon hosts the always-available menu-bar controls (spec §6.1):
//! "Open Hotwire…" reveals the configuration window, "Pause capture" toggles
//! the fail-open pause (a recovery surface available without opening the main
//! window), and "Quit" exits the app. A full HTML popover panel that shows
//! live status is a follow-up; the control surface and the lifecycle it drives
//! live here.

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager,
};

/// Stable tray identifier, used to look the icon up later.
const TRAY_ID: &str = "hotwire-tray";
/// Menu id for the "Open Hotwire…" item.
const MENU_OPEN: &str = "open";
/// Menu id for the "Quit" item.
const MENU_QUIT: &str = "quit";

/// Builds the menu-bar icon and its controls.
///
/// `pause` is the shared pause item owned by the shell state, so its label
/// stays in sync with the pause commands and the emergency bypass.
///
/// # Errors
///
/// Returns the underlying Tauri error when the menu or the tray icon cannot
/// be created.
pub fn setup_tray(app: &AppHandle, pause: &MenuItem<tauri::Wry>) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, MENU_OPEN, "Open Hotwire…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "Quit", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, pause, &separator, &quit])?;

    TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .icon(
            app.default_window_icon()
                .expect("bundle icon is embedded")
                .clone(),
        )
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_OPEN => {
                let _ = crate::window::show_main_window(app);
            }
            crate::state::MENU_PAUSE => {
                let state = app.state::<crate::state::ShellState>();
                // Pause/resume drives both recovery surfaces: capture on the
                // tap and every active adapter hold (fail-open, spec §15.5).
                if state.is_paused() {
                    state.resume();
                } else {
                    let _ = tauri::async_runtime::block_on(state.pause());
                }
            }
            MENU_QUIT => app.exit(0),
            _ => {}
        })
        .build(app)?;

    Ok(())
}
