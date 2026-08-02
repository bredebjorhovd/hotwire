//! Configuration-window lifecycle.
//!
//! Hotwire is a menu-bar app (spec §6.1): the main window is the
//! configuration surface and closing it must keep the app running so capture
//! and the menu-bar controls survive. Quitting happens explicitly through the
//! menu bar or `quit`.

use tauri::{AppHandle, Manager, Window, WindowEvent};

/// Label of the configuration window, matching `tauri.conf.json`.
pub const MAIN_WINDOW_LABEL: &str = "main";

/// Reveals and focuses the configuration window.
///
/// Used by the menu-bar "Open Hotwire…" item, the `show_main_window` command,
/// and the macOS Dock-reopen event. A hidden window is shown and any focus
/// stealing from the menu bar is undone.
pub fn show_main_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        if window.is_minimized()? {
            window.unminimize()?;
        }
        if !window.is_visible()? {
            window.show()?;
        }
        window.set_focus()?;
    }
    Ok(())
}

/// Window-event hook: turns the red close button into a hide.
///
/// A menu-bar app keeps running without a visible window; `Quit` from the
/// tray is the only way out. `AppHandle::exit` bypasses this event, so the
/// actual quit path is never intercepted.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        api.prevent_close();
        let _ = window.hide();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_window_label_matches_config() {
        assert_eq!(MAIN_WINDOW_LABEL, "main");
    }
}
