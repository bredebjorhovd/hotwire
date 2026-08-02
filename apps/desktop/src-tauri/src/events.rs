//! Typed Tauri event boundary.
//!
//! Events are the shell-to-UI half of the typed IPC surface: Rust emits
//! strongly typed payloads (from `hotwire-core`) that the frontend bridge
//! (`apps/desktop/src/features/bridge/ipc.ts`) subscribes to. The event name
//! is the single source of truth for both sides.

use hotwire_core::ActionReceipt;
use tauri::{AppHandle, Emitter};

/// Event emitted whenever an [`ActionReceipt`] is produced.
///
/// Mirrored by `ACTION_RECEIPT_EVENT` in the frontend bridge.
pub const ACTION_RECEIPT_EVENT: &str = "action-receipt";

/// Broadcasts an action receipt to every webview (live board, logs).
///
/// # Errors
///
/// Returns the underlying Tauri error when the event cannot be emitted.
pub fn emit_action_receipt(app: &AppHandle, receipt: &ActionReceipt) -> tauri::Result<()> {
    app.emit(ACTION_RECEIPT_EVENT, receipt.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_event_name_is_stable() {
        assert_eq!(ACTION_RECEIPT_EVENT, "action-receipt");
    }
}
