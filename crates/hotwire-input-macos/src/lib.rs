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
use std::time::Duration;

use core_graphics::event::{
    CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
};
use hotwire_core::{CaptureStatus, PhysicalKeyEvent};
use hotwire_input::{BackendError, InputBackend};

pub use crate::inject::{InjectError, MacEventInjector, INJECTED_MARKER};
pub use crate::keycode::{from_physical_name, is_numpad, physical_name};
pub use crate::normalize::{is_injected, modifier_state, normalize_event};
pub use crate::tap::{TapDecision, TapStatus};
pub use hotwire_core::CaptureHealth;
pub use hotwire_input::{
    BypassAction, CaptureGate, CaptureMode, CapturePolicy, EmergencyBypass, GateDecision,
    ModifierChord, PermissionStatus,
};

/// macOS Accessibility/Input Monitoring trust for this process.
///
/// The [`PermissionStatus`] type lives in `hotwire-core` (re-exported through
/// `hotwire-input`) so every backend and the diagnostics surface share one
/// model; this crate re-exports it.
///
/// The macOS Quartz event-tap backend.
///
/// A [`QuartzEventTap`] owns one tap thread. Configure it, call
/// [`start`](Self::start) with the normalized-event channel, and later
/// [`stop`](Self::stop) it; `stop` also releases any logically held keys.
pub struct QuartzEventTap {
    shared: Arc<tap::TapShared>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

/// How long [`QuartzEventTap::start`] waits for the tap thread to come up
/// before failing the startup handshake.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(5);

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

    /// Returns a diagnostics snapshot of capture health.
    ///
    /// Combines the live permission state, tap status, and pause flag into the
    /// neutral [`CaptureHealth`] model that diagnostics and the fail-open gate
    /// consume. The snapshot carries status categories only — never typed
    /// text, prompts, secrets, or key sequences.
    #[must_use]
    pub fn health(&self) -> CaptureHealth {
        CaptureHealth {
            permission: self.permission_status(),
            status: match self.status() {
                TapStatus::Stopped => CaptureStatus::Stopped,
                TapStatus::Running => CaptureStatus::Running,
                TapStatus::DisabledByTimeout => CaptureStatus::DisabledByTimeout,
                TapStatus::DisabledByUserInput => CaptureStatus::DisabledByUserInput,
                TapStatus::StartFailed => CaptureStatus::StartFailed,
            },
            paused: self.is_paused(),
        }
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
    /// This is a synchronous startup: it waits until the tap thread reports
    /// that the event tap is running, so a failure to create the tap or its
    /// run-loop source surfaces here as an error (and the thread slot is
    /// cleaned up, allowing a retry).
    ///
    /// # Errors
    ///
    /// Returns [`BackendError::Start`] when the process lacks Accessibility
    /// trust, the tap is already running, or the tap thread failed to come up.
    pub fn start(&self, sink: Sender<PhysicalKeyEvent>) -> Result<(), BackendError> {
        if !ffi::process_is_trusted() {
            return Err(BackendError::Start(
                "Accessibility/Input Monitoring permission is not granted; \
                 enable it in System Settings > Privacy & Security > Accessibility"
                    .to_string(),
            ));
        }

        let callback_shared = Arc::clone(&self.shared);
        self.start_with_factory(sink, move || {
            CGEventTap::new(
                CGEventTapLocation::HID,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::Default,
                vec![
                    CGEventType::KeyDown,
                    CGEventType::KeyUp,
                    CGEventType::FlagsChanged,
                ],
                move |_proxy, etype, event| tap::tap_callback(&callback_shared, etype, event),
            )
        })
    }

    /// `start` with the tap factory injected, so tests can exercise the
    /// startup handshake without needing an OS permission failure.
    fn start_with_factory(
        &self,
        sink: Sender<PhysicalKeyEvent>,
        create_tap: impl FnOnce() -> Result<CGEventTap<'static>, ()> + Send + 'static,
    ) -> Result<(), BackendError> {
        if lock(&self.thread).is_some() {
            return Err(BackendError::Start(
                "event tap is already running".to_string(),
            ));
        }

        *lock(&self.shared.sink) = Some(sink);
        self.shared.stop_requested.store(false, Ordering::Release);

        let (startup_tx, startup_rx) = std::sync::mpsc::channel();
        let shared = Arc::clone(&self.shared);
        let handle = thread::Builder::new()
            .name("hotwire-quartz-tap".to_string())
            .spawn(move || tap::run_tap_thread(shared, startup_tx, create_tap))
            .map_err(|error| BackendError::Start(format!("failed to spawn tap thread: {error}")))?;
        *lock(&self.thread) = Some(handle);

        match startup_rx.recv_timeout(STARTUP_TIMEOUT) {
            Ok(tap::StartupStatus::Ready) => Ok(()),
            Ok(tap::StartupStatus::Failed(reason)) => {
                if let Some(handle) = lock(&self.thread).take() {
                    let _ = handle.join();
                }
                Err(BackendError::Start(reason))
            }
            Err(_) => {
                self.shared.stop_requested.store(true, Ordering::Release);
                if let Some(handle) = lock(&self.thread).take() {
                    let _ = handle.join();
                }
                Err(BackendError::Start(
                    "timed out waiting for the event tap to start".to_string(),
                ))
            }
        }
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

    #[test]
    fn a_fresh_gate_captures_no_keys_by_default() {
        let tap = macos_backend();

        assert!(
            lock(&tap.shared.gate).policy().captured_keys().is_empty(),
            "a fresh gate must not contain a placeholder empty-string key"
        );
    }

    #[test]
    fn health_reflects_tap_state_and_fails_open_on_secure_input() {
        let tap = macos_backend();

        let stopped = tap.health();
        assert_eq!(stopped.status, hotwire_core::CaptureStatus::Stopped);
        assert!(stopped.fail_open(), "a stopped tap must fail open");
        assert!(!stopped.ready());

        // A secure-input disable is fail-open; a timeout disable recovers.
        *lock(&tap.shared.status) = crate::tap::TapStatus::DisabledByUserInput;
        assert!(tap.health().fail_open());
        *lock(&tap.shared.status) = crate::tap::TapStatus::DisabledByTimeout;
        assert!(!tap.health().fail_open());
        assert!(!tap.health().ready());
    }

    #[test]
    fn startup_failure_returns_error_and_clears_the_thread_slot() {
        let tap = macos_backend();
        let (sink, _rx) = std::sync::mpsc::channel();

        let error = tap
            .start_with_factory(sink, || Err(()))
            .expect_err("a failing tap factory must surface synchronously");
        assert!(matches!(error, BackendError::Start(_)));
        assert!(
            lock(&tap.thread).is_none(),
            "the thread slot must be cleaned up after a failed start"
        );

        let (sink, _rx) = std::sync::mpsc::channel();
        let error = tap
            .start_with_factory(sink, || Err(()))
            .expect_err("a retry must be allowed after a failed start");
        assert!(matches!(error, BackendError::Start(_)));
    }
}
