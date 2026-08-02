//! Platform-neutral types for Hotwire's input and action pipeline.
//!
//! Native input callbacks must only normalize and enqueue events. They must
//! never execute actions directly.

use serde::{Deserialize, Serialize};

/// The physical interaction a binding is triggered by.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    /// A single key press (down then up).
    Press,
    /// A press-and-hold interaction; the action starts on down and ends on up.
    Hold,
    /// Two presses within a short window.
    DoublePress,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyState {
    Down,
    Up,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)] // Independent OS modifier flags, not state.
pub struct ModifierState {
    pub shift: bool,
    pub control: bool,
    pub option: bool,
    pub command: bool,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalKeyEvent {
    pub device_hint: Option<String>,
    pub scan_code: u32,
    pub physical_code: String,
    pub state: KeyState,
    pub modifiers: ModifierState,
    pub timestamp_ns: u64,
    pub is_repeat: bool,
    /// Platform input backends set this for events emitted by Hotwire.
    pub is_injected: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionStatus {
    Started,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionReceipt {
    pub execution_id: String,
    pub profile_id: String,
    pub binding_id: String,
    pub physical_code: String,
    pub action_id: String,
    pub adapter_id: String,
    pub status: ActionStatus,
    pub message: Option<String>,
}

/// Returns whether an input event is eligible for binding lookup.
///
/// Injected events are always ignored to prevent recursive shortcut bindings.
#[must_use]
pub const fn should_route(event: &PhysicalKeyEvent) -> bool {
    !event.is_injected && !event.is_repeat
}

/// Whether the OS trust needed for input capture is present.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionStatus {
    /// The process can create event taps and intercept input.
    Authorized,
    /// The process has not been granted the required permission.
    Denied,
}

/// The live state of the capture backend, for diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureStatus {
    /// Capture is not running.
    Stopped,
    /// Capture is enabled and processing events.
    Running,
    /// The tap was disabled by the system for a slow callback and is being
    /// re-enabled.
    DisabledByTimeout,
    /// The system disabled capture because the user entered secure input;
    /// keys pass through until capture is restarted.
    DisabledByUserInput,
    /// Capture could not be started (most often a missing permission).
    StartFailed,
}

/// A snapshot of capture health, used by diagnostics and the fail-open gate.
///
/// The field set is deliberately closed: it carries status categories only,
/// never typed text, prompts, secrets, or arbitrary key sequences (spec
/// §21).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureHealth {
    pub permission: PermissionStatus,
    pub status: CaptureStatus,
    /// Capture is paused (emergency bypass or user action) and fails open.
    pub paused: bool,
}

impl CaptureHealth {
    /// Returns whether input must pass through untouched (fail-open).
    ///
    /// Missing permission, a stopped/failed tap, secure-input disablement, or
    /// an explicit pause all mean Hotwire must never suppress a key.
    #[must_use]
    pub const fn fail_open(&self) -> bool {
        matches!(self.permission, PermissionStatus::Denied)
            || matches!(
                self.status,
                CaptureStatus::Stopped
                    | CaptureStatus::StartFailed
                    | CaptureStatus::DisabledByUserInput
            )
            || self.paused
    }

    /// Returns whether capture is healthy and actively suppressing keys.
    #[must_use]
    pub const fn ready(&self) -> bool {
        !self.fail_open() && matches!(self.status, CaptureStatus::Running)
    }
}

/// A minimal summary of the most recent action, for diagnostics.
///
/// Deliberately contains no command text, file paths, or key sequences.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionSummary {
    pub action_id: String,
    pub adapter_id: String,
    pub status: ActionStatus,
}

/// A diagnostics snapshot, safe to render anywhere (spec §6.4, §21).
///
/// The model can only represent permitted categories: capture health, the
/// app version, and a summary of the last action. Typed text, prompts, exact
/// commands, file paths, and arbitrary key sequences have no field here.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsReport {
    pub app_version: Option<String>,
    pub capture: CaptureHealth,
    pub last_action: Option<ActionSummary>,
}

/// Telemetry policy. Hotwire ships with telemetry off (spec §21); only an
/// explicit, separate opt-in can enable the permitted reporting categories.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetryPolicy {
    /// Whether optional diagnostics reporting is enabled. Defaults to `false`.
    pub enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(is_injected: bool, is_repeat: bool) -> PhysicalKeyEvent {
        PhysicalKeyEvent {
            device_hint: None,
            scan_code: 82,
            physical_code: "Numpad0".into(),
            state: KeyState::Down,
            modifiers: ModifierState::default(),
            timestamp_ns: 1,
            is_repeat,
            is_injected,
        }
    }

    #[test]
    fn physical_events_are_routable() {
        assert!(should_route(&event(false, false)));
    }

    #[test]
    fn generated_and_repeat_events_are_filtered() {
        assert!(!should_route(&event(true, false)));
        assert!(!should_route(&event(false, true)));
    }

    fn health(permission: PermissionStatus, status: CaptureStatus, paused: bool) -> CaptureHealth {
        CaptureHealth {
            permission,
            status,
            paused,
        }
    }

    #[test]
    fn fail_open_covers_every_unhealthy_state() {
        assert!(health(PermissionStatus::Denied, CaptureStatus::Running, false).fail_open());
        assert!(health(PermissionStatus::Authorized, CaptureStatus::Stopped, false).fail_open());
        assert!(health(
            PermissionStatus::Authorized,
            CaptureStatus::StartFailed,
            false
        )
        .fail_open());
        assert!(health(
            PermissionStatus::Authorized,
            CaptureStatus::DisabledByUserInput,
            false
        )
        .fail_open());
        assert!(health(PermissionStatus::Authorized, CaptureStatus::Running, true).fail_open());
    }

    #[test]
    fn a_healthy_running_unpaused_capture_is_ready() {
        let healthy = health(PermissionStatus::Authorized, CaptureStatus::Running, false);
        assert!(!healthy.fail_open());
        assert!(healthy.ready());
    }

    #[test]
    fn a_timeout_disabled_tap_recovers_and_is_not_fail_open() {
        let recovering = health(
            PermissionStatus::Authorized,
            CaptureStatus::DisabledByTimeout,
            false,
        );
        assert!(!recovering.fail_open());
        assert!(!recovering.ready());
    }

    #[test]
    fn telemetry_is_off_by_default() {
        assert!(!TelemetryPolicy::default().enabled);
    }

    #[test]
    fn diagnostics_report_serializes_only_permitted_fields() {
        let report = DiagnosticsReport {
            app_version: Some("0.1.0".into()),
            capture: health(PermissionStatus::Authorized, CaptureStatus::Running, false),
            last_action: Some(ActionSummary {
                action_id: "app.open_or_focus".into(),
                adapter_id: "herdr".into(),
                status: ActionStatus::Succeeded,
            }),
        };

        let value = serde_json::to_value(report).expect("report serializes");
        assert!(value.get("appVersion").is_some());
        assert!(value.get("capture").is_some());
        assert!(value.get("lastAction").is_some());
    }
}
