//! Typed Tauri IPC commands.
//!
//! Every command crosses a declared domain boundary: `app_status` reports the
//! shell/build state and `validate_profile` runs the profile-validation
//! boundary from `hotwire-profile`. Frontend-side typed wrappers live in
//! `apps/desktop/src/features/bridge/ipc.ts`.

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
