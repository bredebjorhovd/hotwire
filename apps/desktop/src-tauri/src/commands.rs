//! Typed Tauri IPC commands.
//!
//! Every command crosses a declared domain boundary: `app_status` reports the
//! shell/build state, `validate_profile` runs the profile-validation boundary
//! from `hotwire-profile`, and the lifecycle commands (`show_main_window`,
//! `quit`) drive the menu-bar/window behavior from the webview. `mock_action_receipt`
//! emits a native `ActionReceipt` event so the frontend can exercise the typed
//! event boundary before real capture lands (INP-001). Frontend-side typed
//! wrappers live in `apps/desktop/src/features/bridge/ipc.ts`.

use hotwire_core::{ActionReceipt, ActionStatus};
use serde::Serialize;

use hotwire_profile::{parse_yaml, Profile, SCHEMA_VERSION};

/// Runtime status reported to the frontend's bridge.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    /// Version of the `hotwire-desktop` shell.
    pub app_version: String,
    /// Profile schema version this build accepts.
    pub profile_schema_version: u32,
    /// Active native input backend, or `"none"` before capture lands.
    pub input_backend: &'static str,
    /// Whether low-level capture is available yet.
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
        input_backend: "none",
        capture_available: false,
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
/// UI can be developed against the same event shape. Also returns the receipt
/// so the caller can inspect it without listening.
#[tauri::command]
pub fn mock_action_receipt(app: tauri::AppHandle) -> ActionReceipt {
    let receipt = mock_receipt();
    let _ = crate::events::emit_action_receipt(&app, &receipt);
    receipt
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
    fn app_status_reports_no_capture_yet() {
        let status = app_status();
        assert_eq!(status.input_backend, "none");
        assert!(!status.capture_available);
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
