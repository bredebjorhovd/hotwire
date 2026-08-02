//! macOS input capture: a Quartz event tap with fail-open safety.
//!
//! This crate turns a `CGEventTap` into the platform-neutral
//! [`hotwire_core::PhysicalKeyEvent`] stream, suppresses only the keys a
//! profile binds while capture is active, passes everything else through, tags
//! and filters Hotwire's own injected events, recovers from tap disables, and
//! provides the `Control` + `Option` + `Command` + `Escape` emergency bypass.
//!
//! Shutdown is fail-open: [`QuartzEventTap::stop`] releases any key Hotwire
//! still holds logically so no key is left down.

mod ffi;
mod inject;
mod keycode;
mod normalize;
mod tap;

use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use hotwire_core::PhysicalKeyEvent;
use hotwire_input::{BackendError, InputBackend};

pub use crate::inject::{InjectError, MacEventInjector, INJECTED_MARKER};
pub use crate::keycode::{from_physical_name, is_numpad, physical_name};
pub use crate::normalize::{is_injected, modifier_state, normalize_event};
pub use crate::tap::{TapDecision, TapStatus};
pub use hotwire_input::{
    BypassAction, CaptureGate, CaptureMode, CapturePolicy, EmergencyBypass, GateDecision,
    ModifierChord,
};

/// macOS Accessibility/Input Monitoring trust for this process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PermissionStatus {
    /// The process can create event taps and post events.
    Authorized,
    /// The process has not been granted the Accessibility permission.
    Denied,
}

/// The macOS Quartz event-tap backend.
///
/// A [`QuartzEventTap`] owns one tap thread. Configure it, call
/// [`start`](Self::start) with the normalized-event channel, and later
/// [`stop`](Self::stop) it; `stop` also releases any logically held keys.
pub struct QuartzEventTap {
    shared: Arc<tap::TapShared>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl QuartzEventTap {
    /// Creates a tap that captures nothing until configured.
    #[must_use]
    pub fn new() -> Self {
        Self {
            shared: Arc::new(tap::TapShared::new()),
            thread: Mutex::new(None),
        }
    }

    /// The stable backend identifier.
    #[must_use]
    pub fn name(&self) -> &'static str {
        "macos-quartz"
    }

    /// Sets whether assigned keys are consumed or merely observed.
    pub fn set_capture_mode(&self, mode: CaptureMode) {
        lock(&self.shared.gate).policy_mut().set_mode(mode);
    }

    /// Replaces the set of physical codes the tap may consume.
    pub fn set_captured_keys(&self, keys: &[String]) {
        lock(&self.shared.gate)
            .policy_mut()
            .set_captured_keys(keys.iter().cloned());
    }

    /// Subscribes an observability channel for per-event decisions.
    ///
    /// The probe harness and the guarded integration test use this to prove
    /// consumption and passthrough; applications may ignore it.
    pub fn set_decision_sink(&self, sink: Sender<TapDecision>) {
        *lock(&self.shared.decisions) = Some(sink);
    }

    /// Pauses capture (fail-open) as if the emergency bypass was pressed.
    pub fn emergency_pause(&self) {
        lock(&self.shared.gate).emergency_pause();
    }

    /// Resumes capture after an emergency pause.
    pub fn emergency_resume(&self) {
        lock(&self.shared.gate).emergency_resume();
    }

    /// Returns whether capture is paused by the emergency bypass.
    #[must_use]
    pub fn is_paused(&self) -> bool {
        lock(&self.shared.gate).is_paused()
    }

    /// Returns the Accessibility trust state for this process.
    #[must_use]
    pub fn permission_status(&self) -> PermissionStatus {
        if ffi::process_is_trusted() {
            PermissionStatus::Authorized
        } else {
            PermissionStatus::Denied
        }
    }

    /// Returns the live tap status.
    #[must_use]
    pub fn status(&self) -> TapStatus {
        *lock(&self.shared.status)
    }

    /// Returns an injector that shares this tap's held-key tracking.
    #[must_use]
    pub fn injector(&self) -> MacEventInjector {
        self.shared.injector.clone()
    }

    /// Releases every key Hotwire currently holds logically.
    ///
    /// Safe to call at any time; `stop` calls it automatically.
    pub fn release_held_keys(&self) {
        let _ = self.shared.injector.release_all();
    }

    /// Starts the capture tap and begins delivering normalized events to
    /// `sink`.
    ///
    /// # Errors
    ///
    /// Returns [`BackendError::Start`] when the process lacks Accessibility
    /// trust or the tap is already running.
    pub fn start(&self, sink: Sender<PhysicalKeyEvent>) -> Result<(), BackendError> {
        if lock(&self.thread).is_some() {
            return Err(BackendError::Start(
                "event tap is already running".to_string(),
            ));
        }
        if !ffi::process_is_trusted() {
            return Err(BackendError::Start(
                "Accessibility/Input Monitoring permission is not granted; \
                 enable it in System Settings > Privacy & Security > Accessibility"
                    .to_string(),
            ));
        }

        *lock(&self.shared.sink) = Some(sink);
        self.shared.stop_requested.store(false, Ordering::Release);

        let shared = Arc::clone(&self.shared);
        let handle = thread::Builder::new()
            .name("hotwire-quartz-tap".to_string())
            .spawn(move || tap::run_tap_thread(shared))
            .map_err(|error| BackendError::Start(format!("failed to spawn tap thread: {error}")))?;

        *lock(&self.thread) = Some(handle);
        Ok(())
    }

    /// Stops the tap and releases any logically held keys.
    ///
    /// Safe to call when the tap is not running. After `stop`, the tap may be
    /// started again.
    pub fn stop(&self) {
        self.shared.stop_requested.store(true, Ordering::Release);
        self.release_held_keys();
        if let Some(runloop) = lock(&self.shared.runloop).as_ref() {
            runloop.stop();
        }
        if let Some(handle) = lock(&self.thread).take() {
            let _ = handle.join();
        }
    }
}

impl Default for QuartzEventTap {
    fn default() -> Self {
        Self::new()
    }
}

impl InputBackend for QuartzEventTap {
    fn name(&self) -> &'static str {
        "macos-quartz"
    }

    fn start(&self, sink: Sender<PhysicalKeyEvent>) -> Result<(), BackendError> {
        QuartzEventTap::start(self, sink)
    }

    fn stop(&self) {
        QuartzEventTap::stop(self);
    }
}

/// Returns the macOS input backend.
///
/// The returned tap captures nothing until configured and started.
#[must_use]
pub fn macos_backend() -> QuartzEventTap {
    QuartzEventTap::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_tap_is_configured_safely() {
        let tap = macos_backend();

        assert_eq!(tap.name(), "macos-quartz");
        assert_eq!(tap.status(), TapStatus::Stopped);
        assert!(!tap.is_paused());
        assert!(tap.injector().held_keys().is_empty());
    }

    #[test]
    fn emergency_pause_and_resume_toggle_the_gate() {
        let tap = macos_backend();

        tap.emergency_pause();
        assert!(tap.is_paused());
        tap.emergency_resume();
        assert!(!tap.is_paused());
    }

    #[test]
    fn captured_keys_and_mode_are_configurable() {
        let tap = macos_backend();

        tap.set_captured_keys(&["Numpad5".to_string()]);
        tap.set_capture_mode(CaptureMode::Passthrough);
        tap.set_capture_mode(CaptureMode::Capture);

        let gate = lock(&tap.shared.gate);
        assert!(gate.policy().captured_keys().contains("Numpad5"));
        assert_eq!(gate.policy().mode(), CaptureMode::Capture);
    }
}
