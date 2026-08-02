//! Shell state and recovery controls.
//!
//! The desktop shell owns one shared [`QuartzEventTap`] (capture is started by
//! the app when permissions allow; until then the gate is inert and fails
//! open), the menu-bar pause item, and the last action receipt. The pause and
//! resume helpers here are the fail-open recovery surface (spec §15.5): they
//! toggle capture on the shared tap and keep the menu-bar item's label in
//! sync. The tap itself owns the gate state, so the helpers are testable
//! without a window.
//!
//! Recovery ownership, by design: when the execution runtime lands (the
//! `hotwire-router` `HotwireRuntime` with its `pause`/`shutdown` cancelling
//! in-flight adapter holds), the shell's pause/quit path must drive *both*
//! surfaces — stop capture on the tap and cancel active executions on the
//! runtime — so no key stays held and no action keeps running. This struct is
//! the single place that ownership will live.

use std::sync::Mutex;

use hotwire_core::ActionReceipt;
use hotwire_input_macos::QuartzEventTap;
use tauri::menu::MenuItem;

/// Menu id for the "Pause capture" / "Resume capture" item.
pub const MENU_PAUSE: &str = "pause";
/// Label shown while capture is running.
pub const PAUSE_LABEL: &str = "Pause capture";
/// Label shown while capture is paused.
pub const RESUME_LABEL: &str = "Resume capture";

/// Process-wide state managed by the Tauri shell.
pub struct ShellState {
    /// The shared macOS capture tap (created but not started by default).
    pub tap: QuartzEventTap,
    /// The menu-bar pause item, so its label tracks the pause state.
    pub pause_item: MenuItem<tauri::Wry>,
    /// The most recent action receipt, for the diagnostics "last action".
    pub last_receipt: Mutex<Option<ActionReceipt>>,
}

/// Creates the menu-bar pause item.
///
/// # Errors
///
/// Returns the underlying Tauri error when the item cannot be created.
pub fn create_pause_item(app: &tauri::AppHandle) -> tauri::Result<MenuItem<tauri::Wry>> {
    MenuItem::with_id(app, MENU_PAUSE, PAUSE_LABEL, true, None::<&str>)
}

/// Pauses capture (fail-open). Returns the new paused state.
pub fn pause_capture(tap: &QuartzEventTap) -> bool {
    tap.emergency_pause();
    tap.is_paused()
}

/// Resumes capture after a pause. Returns the new paused state.
pub fn resume_capture(tap: &QuartzEventTap) -> bool {
    tap.emergency_resume();
    tap.is_paused()
}

/// Toggles capture pause. Returns the new paused state.
pub fn toggle_pause(tap: &QuartzEventTap) -> bool {
    if tap.is_paused() {
        resume_capture(tap)
    } else {
        pause_capture(tap)
    }
}

/// Re-labels the menu-bar pause item to match the paused state.
pub fn sync_pause_label(pause_item: &MenuItem<tauri::Wry>, paused: bool) {
    let _ = pause_item.set_text(if paused { RESUME_LABEL } else { PAUSE_LABEL });
}

/// Maps the last action receipt into the diagnostics summary.
#[must_use]
pub fn summarize_last_receipt(
    receipt: &Option<ActionReceipt>,
) -> Option<hotwire_core::ActionSummary> {
    receipt.as_ref().map(|receipt| hotwire_core::ActionSummary {
        action_id: receipt.action_id.clone(),
        adapter_id: receipt.adapter_id.clone(),
        status: receipt.status.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_and_resume_toggle_the_shared_tap() {
        let tap = QuartzEventTap::new();
        assert!(!tap.is_paused());

        assert!(pause_capture(&tap));
        assert!(tap.is_paused());
        assert!(tap.health().fail_open(), "pausing must fail open");

        assert!(!resume_capture(&tap));
        assert!(!tap.is_paused());
    }

    #[test]
    fn toggle_flips_between_pause_and_resume() {
        let tap = QuartzEventTap::new();

        assert!(toggle_pause(&tap));
        assert!(tap.is_paused());
        assert!(!toggle_pause(&tap));
        assert!(!tap.is_paused());
    }

    #[test]
    fn pause_labels_are_stable() {
        assert_eq!(MENU_PAUSE, "pause");
        assert_eq!(PAUSE_LABEL, "Pause capture");
        assert_eq!(RESUME_LABEL, "Resume capture");
    }

    #[test]
    fn last_receipt_maps_to_a_summary_without_sensitive_detail() {
        let receipt = crate::commands::mock_receipt();
        let summary = summarize_last_receipt(&Some(receipt)).expect("summary");
        assert_eq!(summary.action_id, "app.open_or_focus");
        assert_eq!(summary.adapter_id, "herdr");
        assert_eq!(summary.status, hotwire_core::ActionStatus::Succeeded);
        assert_eq!(summarize_last_receipt(&None), None);
    }
}
