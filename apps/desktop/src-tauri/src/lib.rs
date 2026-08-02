//! Hotwire desktop shell.
//!
//! Thin Tauri 2 glue: it registers the typed IPC commands the React frontend
//! calls and owns the application lifecycle. All domain logic lives in the
//! `crates/` workspace members; this crate only exposes those boundaries over
//! IPC. Native input capture is intentionally absent (BOOT-001 scope).

// Tauri's macro-generated context is not written to satisfy clippy::pedantic.
#![allow(clippy::pedantic)]

mod commands;

/// Runs the desktop application.
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::app_status,
            commands::validate_profile
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Hotwire desktop shell");
}
