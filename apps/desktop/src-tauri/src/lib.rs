//! Hotwire desktop shell.
//!
//! Thin Tauri 2 glue: it registers the typed IPC commands the React frontend
//! calls, owns the configuration-window lifecycle, and hosts the menu-bar
//! controls (spec §6.1). All domain logic lives in the `crates/` workspace
//! members; this crate only exposes those boundaries over IPC. Native input
//! capture is intentionally absent (INP-001 scope); `mock_action_receipt`
//! exercises the typed event boundary in the meantime.

// Tauri's macro-generated context is not written to satisfy clippy::pedantic.
#![allow(clippy::pedantic)]

mod commands;
mod events;
mod tray;
mod window;

/// Runs the desktop application.
pub fn run() {
    let app = tauri::Builder::default()
        .setup(|app| {
            tray::setup_tray(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            window::on_window_event(window, event);
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_status,
            commands::validate_profile,
            commands::show_main_window,
            commands::quit,
            commands::mock_action_receipt
        ])
        .build(tauri::generate_context!())
        .expect("error while building the Hotwire desktop shell");

    app.run(|app_handle, event| {
        // Clicking the Dock icon on macOS re-opens the configuration window.
        if let tauri::RunEvent::Reopen { .. } = event {
            let _ = window::show_main_window(app_handle);
        }
    });
}
