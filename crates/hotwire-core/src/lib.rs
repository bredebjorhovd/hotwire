//! Platform-neutral types for Hotwire's input and action pipeline.
//!
//! Native input callbacks must only normalize and enqueue events. They must
//! never execute actions directly.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
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
}
