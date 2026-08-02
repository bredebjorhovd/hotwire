//! Typed Tauri IPC commands.
//!
//! Every command crosses a declared domain boundary: `app_status` reports the
//! shell/build state, `validate_profile` runs the profile-validation boundary
//! from `hotwire-profile`, and the lifecycle commands (`show_main_window`,
//! `quit`) drive the menu-bar/window behavior from the webview. `mock_action_receipt`
//! emits a native `ActionReceipt` event so the frontend can exercise the typed
//! event boundary before real capture lands (INP-001). Frontend-side typed
//! wrappers live in `apps/desktop/src/features/bridge/ipc.ts`.

use hotwire_core::{ActionReceipt, ActionStatus, DiagnosticsReport, Trigger};
use serde::Serialize;
use serde_json::Value;

use hotwire_profile::{parse_yaml, Profile, SCHEMA_VERSION};

/// Runtime status reported to the frontend's bridge.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    /// Version of the `hotwire-desktop` shell.
    pub app_version: String,
    /// Profile schema version this build accepts.
    pub profile_schema_version: u32,
    /// Active native input backend.
    pub input_backend: &'static str,
    /// Whether the native capture backend is compiled into this shell.
    pub capture_available: bool,
}

/// Result of validating a profile document over IPC.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileValidationReport {
    pub valid: bool,
    pub profile: Option<Profile>,
    pub error: Option<String>,
}

/// Returns the desktop shell's runtime status.
#[tauri::command]
pub fn app_status() -> AppStatus {
    AppStatus {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        profile_schema_version: SCHEMA_VERSION,
        input_backend: "macos-quartz",
        capture_available: true,
    }
}

/// Validates a YAML profile document against the current schema.
#[tauri::command]
pub fn validate_profile(yaml: String) -> ProfileValidationReport {
    match parse_yaml(&yaml) {
        Ok(profile) => ProfileValidationReport {
            valid: true,
            profile: Some(profile),
            error: None,
        },
        Err(error) => ProfileValidationReport {
            valid: false,
            profile: None,
            error: Some(error.to_string()),
        },
    }
}

/// Activates a validated profile for the native event router.
#[tauri::command]
pub fn activate_profile(
    yaml: String,
    state: tauri::State<'_, crate::state::ShellState>,
) -> Result<(), String> {
    let profile = parse_yaml(&yaml).map_err(|error| error.to_string())?;
    state.activate_profile(profile)
}

/// Reveals and focuses the configuration window.
///
/// # Errors
///
/// Returns a readable error when the window cannot be shown or focused.
#[tauri::command]
pub fn show_main_window(app: tauri::AppHandle) -> Result<(), String> {
    crate::window::show_main_window(&app).map_err(|error| error.to_string())
}

/// Quits the desktop shell cleanly.
#[tauri::command]
pub fn quit(app: tauri::AppHandle) {
    app.exit(0);
}

/// Emits a mocked [`ActionReceipt`] event (spec §10.1 typed events).
///
/// Stands in for a real `hotwire-core` run until native capture lands, so the
/// UI can be developed against the same event shape. Also records the receipt
/// as the diagnostics "last action" and returns it so the caller can inspect
/// it without listening.
#[tauri::command]
pub fn mock_action_receipt(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::state::ShellState>,
) -> ActionReceipt {
    let receipt = mock_receipt();
    if let Ok(mut last) = state.last_receipt.lock() {
        *last = Some(receipt.clone());
    }
    let _ = crate::events::emit_action_receipt(&app, &receipt);
    receipt
}

/// Returns a diagnostics snapshot (spec §6.4).
///
/// The report is deliberately restricted to permitted categories: capture
/// health, app version, and a summary of the last action. It never contains
/// typed text, prompts, exact commands, or arbitrary key sequences (spec §21).
#[tauri::command]
pub fn diagnostics(state: tauri::State<'_, crate::state::ShellState>) -> DiagnosticsReport {
    let last_action = state
        .last_receipt
        .lock()
        .ok()
        .and_then(|guard| crate::state::summarize_last_receipt(&guard));
    DiagnosticsReport {
        app_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        capture: state.tap.health(),
        last_action,
    }
}

/// Pauses the shell (fail-open recovery surface): stops capture on the tap
/// and cancels/releases every active adapter hold so no key stays logically
/// held. Returns the new paused state.
///
/// # Errors
///
/// Returns a readable error when the pause cannot complete.
#[tauri::command]
pub async fn pause_capture(
    state: tauri::State<'_, crate::state::ShellState>,
) -> Result<bool, String> {
    let _ = state.pause().await;
    Ok(state.tap.is_paused())
}

/// Resumes the shell after a pause. Returns the new paused state.
///
/// # Errors
///
/// Returns a readable error when the resume cannot complete.
#[tauri::command]
pub async fn resume_capture(
    state: tauri::State<'_, crate::state::ShellState>,
) -> Result<bool, String> {
    Ok(state.resume().await)
}

/// Runs one action through a registered adapter and emits the resulting
/// receipt to the UI (ADP-001 vertical slice).
///
/// This is the live half of the slice: a `hold` execution that reports
/// `Started` stays tracked so the UI can end it with `release_adapter_action`
/// or cancel it with `cancel_adapter_action`.
#[tauri::command]
pub async fn run_adapter_action(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::state::ShellState>,
    adapter_id: String,
    action_id: String,
    trigger: Trigger,
    config: Value,
    physical_code: String,
) -> Result<ActionReceipt, String> {
    let receipt = state
        .adapters
        .run(&adapter_id, &action_id, trigger, config, &physical_code)
        .await;
    let _ = crate::events::emit_action_receipt(&app, &receipt);
    Ok(receipt)
}

/// Ends a tracked hold execution (e.g. releasing a Papegøye push-to-talk key)
/// and emits its completion receipt.
#[tauri::command]
pub async fn release_adapter_action(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::state::ShellState>,
    adapter_id: String,
    execution_id: String,
    physical_code: String,
) -> Result<ActionReceipt, String> {
    let receipt = state
        .adapters
        .release(&adapter_id, &execution_id, &physical_code)
        .await;
    let _ = crate::events::emit_action_receipt(&app, &receipt);
    Ok(receipt)
}

/// Cancels a tracked execution and emits its `Cancelled` receipt.
#[tauri::command]
pub async fn cancel_adapter_action(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::state::ShellState>,
    adapter_id: String,
    execution_id: String,
    physical_code: String,
) -> Result<ActionReceipt, String> {
    let receipt = state
        .adapters
        .cancel(&adapter_id, &execution_id, &physical_code)
        .await;
    let _ = crate::events::emit_action_receipt(&app, &receipt);
    Ok(receipt)
}

/// Probes one registered adapter for machine-level presence.
///
/// # Errors
///
/// Returns an error when the adapter is not registered.
#[tauri::command]
pub async fn detect_adapter(
    state: tauri::State<'_, crate::state::ShellState>,
    adapter_id: String,
) -> Result<hotwire_adapter_sdk::DetectionResult, String> {
    state.adapters.detect(&adapter_id).await
}

/// Validates a binding config against one registered adapter.
///
/// # Errors
///
/// Returns an error when the adapter is not registered.
#[tauri::command]
pub async fn validate_adapter_config(
    state: tauri::State<'_, crate::state::ShellState>,
    adapter_id: String,
    config: Value,
) -> Result<hotwire_adapter_sdk::ValidationResult, String> {
    state.adapters.validate_config(&adapter_id, &config).await
}

/// The fixture receipt used by `mock_action_receipt`.
///
/// Mirrors the first vertical slice (`Numpad5 → OPEN_HERDR → herdr`) so the
/// event path is exercised with a realistic payload.
#[must_use]
pub fn mock_receipt() -> ActionReceipt {
    ActionReceipt {
        execution_id: "mock-001".to_string(),
        profile_id: "ai-numpad".to_string(),
        binding_id: "b-numpad5-herdr".to_string(),
        physical_code: "Numpad5".to_string(),
        action_id: "app.open_or_focus".to_string(),
        adapter_id: "herdr".to_string(),
        status: ActionStatus::Succeeded,
        message: Some("Focused Herdr".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_receipt_matches_the_first_slice() {
        let receipt = mock_receipt();
        assert_eq!(receipt.physical_code, "Numpad5");
        assert_eq!(receipt.action_id, "app.open_or_focus");
        assert_eq!(receipt.adapter_id, "herdr");
        assert_eq!(receipt.status, ActionStatus::Succeeded);
    }

    #[test]
    fn app_status_reports_native_capture_backend() {
        let status = app_status();
        assert_eq!(status.input_backend, "macos-quartz");
        assert!(status.capture_available);
        assert_eq!(status.profile_schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn validate_profile_rejects_invalid_yaml() {
        let report = validate_profile("bindings: [not a profile]".to_string());
        assert!(!report.valid);
        assert!(report.profile.is_none());
        assert!(report.error.is_some());
    }

    #[test]
    fn receipt_serializes_camel_case() {
        let receipt = mock_receipt();
        let json = serde_json::to_value(receipt).expect("receipt serializes");
        assert!(json.get("executionId").is_some());
        assert!(json.get("physicalCode").is_some());
        assert!(json.get("actionId").is_some());
    }
}
