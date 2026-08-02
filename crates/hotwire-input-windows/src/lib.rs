//! Windows input capture boundary.
//!
//! The full implementation uses `SetWindowsHookEx` with `WH_KEYBOARD_LL` and a
//! `LowLevelKeyboardProc` callback to observe and suppress low-level keyboard
//! events. This ticket only establishes the seam; the low-level hook lands
//! behind a feature flag after the macOS proof (INP-001) settles the
//! normalization contract. Windows is not advertised as stable.

use std::sync::mpsc::Sender;

use hotwire_core::PhysicalKeyEvent;
use hotwire_input::{BackendError, InputBackend};

/// Placeholder backend for the Windows low-level keyboard hook.
///
/// Returned by [`windows_backend`] until a Windows implementation exists.
pub struct LowLevelKeyboardHook;

impl InputBackend for LowLevelKeyboardHook {
    fn name(&self) -> &'static str {
        "windows-low-level-hook"
    }

    fn start(&self, _sink: Sender<PhysicalKeyEvent>) -> Result<(), BackendError> {
        Err(BackendError::NotImplemented(self.name()))
    }
}

/// Returns the Windows input backend.
///
/// The returned backend currently refuses to start; Windows support is gated
/// behind a feature flag and is not part of the v0.1 macOS milestone.
#[must_use]
pub fn windows_backend() -> LowLevelKeyboardHook {
    LowLevelKeyboardHook
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_seam_exists_but_is_not_implemented() {
        let (sender, _receiver) = std::sync::mpsc::channel();
        let backend = windows_backend();

        assert_eq!(backend.name(), "windows-low-level-hook");
        assert!(matches!(
            backend.start(sender),
            Err(BackendError::NotImplemented("windows-low-level-hook"))
        ));
    }
}
