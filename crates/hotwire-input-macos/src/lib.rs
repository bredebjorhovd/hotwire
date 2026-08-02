//! macOS input capture boundary.
//!
//! The full implementation uses a Quartz Event Tap (`CGEventTap`) to observe and
//! suppress low-level keyboard events, and delivers normalized
//! [`hotwire_core::PhysicalKeyEvent`] values through the
//! [`hotwire_input::InputBackend`] seam. This ticket only establishes the
//! seam; no capture code is implemented here yet. The event tap arrives with
//! the macOS input proof (INP-001).

use std::sync::mpsc::Sender;

use hotwire_core::PhysicalKeyEvent;
use hotwire_input::{BackendError, InputBackend};

/// Placeholder backend for the macOS Quartz event tap.
///
/// Returned by [`macos_backend`] until the real event-tap implementation
/// lands in INP-001.
pub struct QuartzEventTap;

impl InputBackend for QuartzEventTap {
    fn name(&self) -> &'static str {
        "macos-quartz"
    }

    fn start(&self, _sink: Sender<PhysicalKeyEvent>) -> Result<(), BackendError> {
        Err(BackendError::NotImplemented(self.name()))
    }
}

/// Returns the macOS input backend.
///
/// The returned backend currently refuses to start; the event-tap
/// implementation is tracked by INP-001.
#[must_use]
pub fn macos_backend() -> QuartzEventTap {
    QuartzEventTap
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_seam_exists_but_is_not_implemented() {
        let (sender, _receiver) = std::sync::mpsc::channel();
        let backend = macos_backend();

        assert_eq!(backend.name(), "macos-quartz");
        assert!(matches!(
            backend.start(sender),
            Err(BackendError::NotImplemented("macos-quartz"))
        ));
    }
}
