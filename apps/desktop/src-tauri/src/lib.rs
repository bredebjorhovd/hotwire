//! Hotwire desktop shell.
//!
//! Thin Tauri 2 glue: it registers the typed IPC commands the React frontend
//! calls, owns the configuration-window lifecycle, hosts the menu-bar
//! controls (spec §6.1), and provides the diagnostics, pause/resume, and
//! fail-open recovery surfaces (SAFE-001). All domain logic lives in the
//! `crates/` workspace members; this crate only exposes those boundaries over
//! IPC. Native Quartz events are drained on a worker thread and routed through
//! the validated Numpad profile; the callback itself never executes an adapter.

// Tauri's macro-generated context is not written to satisfy clippy::pedantic.
#![allow(clippy::pedantic)]

mod adapters;
mod commands;
mod events;
mod state;
mod tray;
mod window;

use hotwire_input_macos::QuartzEventTap;
use tauri::Manager;

/// Runs the desktop application.
pub fn run() {
    let app = tauri::Builder::default()
        .setup(|app| {
            let pause_item = state::create_pause_item(app.handle())?;
            app.manage(state::ShellState::new(
                QuartzEventTap::new(),
                pause_item.clone(),
                adapters::AdapterState::new(),
            ));
            let state = app.state::<state::ShellState>();
            state.configure_capture();
            let (event_tx, event_rx) = std::sync::mpsc::channel();
            if state.tap.start(event_tx).is_ok() {
                let handle = app.handle().clone();
                std::thread::Builder::new()
                    .name("hotwire-input-router".to_string())
                    .spawn(move || {
                        while let Ok(event) = event_rx.recv() {
                            let state = handle.state::<state::ShellState>();
                            tauri::async_runtime::block_on(state.route_event(&handle, event));
                        }
                    })
                    .expect("input router thread should start");
            }
            tray::setup_tray(app.handle(), &pause_item)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            window::on_window_event(window, event);
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_status,
            commands::validate_profile,
            commands::activate_profile,
            commands::show_main_window,
            commands::quit,
            commands::mock_action_receipt,
            commands::diagnostics,
            commands::pause_capture,
            commands::resume_capture,
            commands::run_adapter_action,
            commands::release_adapter_action,
            commands::cancel_adapter_action,
            commands::detect_adapter,
            commands::validate_adapter_config
        ])
        .build(tauri::generate_context!())
        .expect("error while building the Hotwire desktop shell");

    app.run(|app_handle, event| {
        match event {
            // Clicking the Dock icon on macOS re-opens the configuration window.
            tauri::RunEvent::Reopen { .. } => {
                let _ = window::show_main_window(app_handle);
            }
            // Clean shutdown (fail-open, spec §15.5): stop capture on the tap
            // and cancel/release every active adapter hold so no key is left
            // logically down when the process exits.
            tauri::RunEvent::Exit => {
                let state = app_handle.state::<state::ShellState>();
                let _ = tauri::async_runtime::block_on(state.shutdown());
            }
            _ => {}
        }
    });
}
