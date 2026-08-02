//! The running Quartz event tap.
//!
//! Threading model: `run_tap_thread` owns the tap and drives its run loop on a
//! dedicated thread. `tap_callback` is invoked by the run loop on that thread
//! for every key event. The callback only normalizes, feeds the pure
//! [`hotwire_input::CaptureGate`], enqueues to channels, and returns a
//! `CallbackResult`; it never executes an action (spec §10.3).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use core_foundation::runloop::CFRunLoop;
use core_graphics::event::{CGEvent, CGEventTap, CGEventType, CallbackResult};
use hotwire_core::PhysicalKeyEvent;
use hotwire_input::CaptureGate;

use crate::ffi;
use crate::inject::MacEventInjector;
use crate::normalize::normalize_event;

/// How often the tap thread wakes to service the run loop and check shutdown.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Recovers a poisoned lock: our critical sections never panic, so a poisoned
/// mutex can only mean a prior test panic, and the values remain usable.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Live state of the capture tap, visible to callers for diagnostics.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TapStatus {
    /// Not started, or cleanly stopped.
    #[default]
    Stopped,
    /// The tap is enabled and processing events.
    Running,
    /// The tap was disabled by the system for a slow callback; Hotwire
    /// re-enables it automatically.
    DisabledByTimeout,
    /// The system disabled the tap because the user entered secure input; the
    /// tap is fail-open until [`crate::QuartzEventTap::stop`] + [`crate::QuartzEventTap::start`].
    DisabledByUserInput,
    /// The tap could not be created (most often missing Accessibility trust).
    StartFailed,
}

/// What the tap did with one normalized event, for observability and tests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TapDecision {
    pub event: PhysicalKeyEvent,
    /// The event was consumed and will not reach other applications.
    pub suppressed: bool,
    /// Capture was paused by the emergency bypass at decision time.
    pub paused: bool,
}

/// The startup handshake the tap thread uses to tell the controller it is
/// either serving events or could not come up.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum StartupStatus {
    /// The tap is enabled and its run loop is running.
    Ready,
    /// The tap could not be created or attached to a run loop.
    Failed(String),
}

/// State shared between the tap thread, the callback, and the controlling
/// [`crate::QuartzEventTap`].
pub(crate) struct TapShared {
    pub gate: Mutex<CaptureGate>,
    pub sink: Mutex<Option<Sender<PhysicalKeyEvent>>>,
    pub decisions: Mutex<Option<Sender<TapDecision>>>,
    pub status: Mutex<TapStatus>,
    pub stop_requested: AtomicBool,
    pub tap_enable_requested: AtomicBool,
    pub runloop: Mutex<Option<CFRunLoop>>,
    pub injector: MacEventInjector,
}

impl TapShared {
    pub fn new() -> Self {
        Self {
            gate: Mutex::new(CaptureGate::new()),
            sink: Mutex::new(None),
            decisions: Mutex::new(None),
            status: Mutex::new(TapStatus::Stopped),
            stop_requested: AtomicBool::new(false),
            tap_enable_requested: AtomicBool::new(false),
            runloop: Mutex::new(None),
            injector: MacEventInjector::default(),
        }
    }
}

impl Default for TapShared {
    fn default() -> Self {
        Self::new()
    }
}

/// Event-tap callback: normalize, gate, enqueue, decide. Must stay short.
///
/// Routing to the action sink is excluded while capture is paused and for the
/// bypass chord event itself, so a paused profile never executes actions and
/// the unremappable chord can never fire an Escape binding. The decision sink
/// always receives every event for observability.
pub(crate) fn tap_callback(
    shared: &TapShared,
    etype: CGEventType,
    event: &CGEvent,
) -> CallbackResult {
    // Out-of-band notifications arrive with a null event; handle them before
    // touching the event.
    match etype {
        CGEventType::TapDisabledByTimeout => {
            shared.tap_enable_requested.store(true, Ordering::Release);
            *lock(&shared.status) = TapStatus::DisabledByTimeout;
            return CallbackResult::Keep;
        }
        CGEventType::TapDisabledByUserInput => {
            // Secure input is in progress; do not fight the system. The tap is
            // fail-open (all keys pass through) until restarted.
            *lock(&shared.status) = TapStatus::DisabledByUserInput;
            return CallbackResult::Keep;
        }
        _ => {}
    }

    let Some(normalized) = normalize_event(event) else {
        return CallbackResult::Keep;
    };

    // Injection-loop prevention: Hotwire's own events always pass through.
    if normalized.is_injected {
        return CallbackResult::Keep;
    }

    let decision = lock(&shared.gate).decide(&normalized);

    // Enqueue outside the callback path; never execute an action here.
    // Paused events and the bypass chord itself must not reach the router.
    if !decision.paused && !decision.bypass_chord {
        if let Some(sink) = lock(&shared.sink).as_ref() {
            let _ = sink.send(normalized.clone());
        }
    }
    if let Some(decisions) = lock(&shared.decisions).as_ref() {
        let _ = decisions.send(TapDecision {
            event: normalized,
            suppressed: decision.suppressed,
            paused: decision.paused,
        });
    }

    if decision.suppressed {
        CallbackResult::Drop
    } else {
        CallbackResult::Keep
    }
}

/// Owns the tap and runs its run loop until shutdown is requested.
///
/// Reports startup success or failure over `startup_tx` so the controller can
/// return a synchronous error instead of leaving a half-started tap behind.
#[allow(clippy::needless_pass_by_value)] // Arc/closure ownership transfers into the spawned thread.
pub(crate) fn run_tap_thread(
    shared: Arc<TapShared>,
    startup_tx: Sender<StartupStatus>,
    create_tap: impl FnOnce() -> Result<CGEventTap<'static>, ()> + Send + 'static,
) {
    let Ok(tap) = create_tap() else {
        *lock(&shared.status) = TapStatus::StartFailed;
        let _ = startup_tx.send(StartupStatus::Failed(
            "failed to create the Quartz event tap (check the Accessibility permission)".into(),
        ));
        return;
    };

    let Ok(source) = tap.mach_port().create_runloop_source(0) else {
        *lock(&shared.status) = TapStatus::StartFailed;
        let _ = startup_tx.send(StartupStatus::Failed(
            "failed to create the event-tap run-loop source".into(),
        ));
        return;
    };

    let runloop = CFRunLoop::get_current();
    runloop.add_source(&source, ffi::common_modes());
    tap.enable();
    *lock(&shared.status) = TapStatus::Running;
    *lock(&shared.runloop) = Some(runloop.clone());
    let _ = startup_tx.send(StartupStatus::Ready);

    while !shared.stop_requested.load(Ordering::Acquire) {
        CFRunLoop::run_in_mode(ffi::default_mode(), POLL_INTERVAL, true);
        if shared.tap_enable_requested.swap(false, Ordering::AcqRel) {
            // Recover from a system timeout: re-enable the tap safely.
            tap.enable();
            *lock(&shared.status) = TapStatus::Running;
        }
    }

    *lock(&shared.status) = TapStatus::Stopped;
    *lock(&shared.runloop) = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_graphics::event::{CGEventFlags, KeyCode};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
    use hotwire_input::CaptureMode;

    fn key_event(keycode: u16, down: bool, flags: CGEventFlags) -> CGEvent {
        let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
            .expect("event source creation should succeed");
        let event = CGEvent::new_keyboard_event(source, keycode, down)
            .expect("keyboard event creation should succeed");
        event.set_flags(flags);
        event
    }

    fn ctrl_opt_cmd() -> CGEventFlags {
        CGEventFlags::CGEventFlagControl
            | CGEventFlags::CGEventFlagAlternate
            | CGEventFlags::CGEventFlagCommand
    }

    fn shared_with_sink() -> (Arc<TapShared>, std::sync::mpsc::Receiver<PhysicalKeyEvent>) {
        let shared = Arc::new(TapShared::new());
        let (tx, rx) = std::sync::mpsc::channel();
        *lock(&shared.sink) = Some(tx);
        (shared, rx)
    }

    #[test]
    fn paused_keys_are_not_routed_to_the_action_sink() {
        let (shared, rx) = shared_with_sink();
        lock(&shared.gate)
            .policy_mut()
            .set_captured_keys(["Numpad5".to_string()]);
        lock(&shared.gate).emergency_pause();

        let event = key_event(KeyCode::ANSI_KEYPAD_5, true, CGEventFlags::default());
        let result = tap_callback(&shared, CGEventType::KeyDown, &event);

        assert!(matches!(result, CallbackResult::Keep));
        assert!(
            rx.try_recv().is_err(),
            "events while paused must not reach the action sink"
        );
    }

    #[test]
    fn the_bypass_chord_is_never_routed_even_with_an_escape_binding() {
        let (shared, rx) = shared_with_sink();
        lock(&shared.gate)
            .policy_mut()
            .set_captured_keys(["Escape".to_string()]);

        let event = key_event(KeyCode::ESCAPE, true, ctrl_opt_cmd());
        let result = tap_callback(&shared, CGEventType::KeyDown, &event);

        assert!(matches!(result, CallbackResult::Keep));
        assert!(
            rx.try_recv().is_err(),
            "the emergency-bypass chord must never be routed as a binding"
        );
        assert!(lock(&shared.gate).is_paused(), "the chord toggles capture");
    }

    #[test]
    fn passthrough_mode_still_routes_without_suppressing() {
        let (shared, rx) = shared_with_sink();
        lock(&shared.gate)
            .policy_mut()
            .set_captured_keys(["Numpad5".to_string()]);
        lock(&shared.gate)
            .policy_mut()
            .set_mode(CaptureMode::Passthrough);

        let event = key_event(KeyCode::ANSI_KEYPAD_5, true, CGEventFlags::default());
        let result = tap_callback(&shared, CGEventType::KeyDown, &event);

        assert!(matches!(result, CallbackResult::Keep));
        let routed = rx
            .try_recv()
            .expect("Passthrough mode must still route events for observation");
        assert_eq!(routed.physical_code, "Numpad5");
        assert!(!routed.is_injected);
        assert!(rx.try_recv().is_err(), "exactly one event expected");
    }

    #[test]
    fn a_bound_key_is_routed_and_suppressed_while_capture_is_active() {
        let (shared, rx) = shared_with_sink();
        lock(&shared.gate)
            .policy_mut()
            .set_captured_keys(["Numpad5".to_string()]);

        let event = key_event(KeyCode::ANSI_KEYPAD_5, true, CGEventFlags::default());
        let result = tap_callback(&shared, CGEventType::KeyDown, &event);

        assert!(matches!(result, CallbackResult::Drop));
        let routed = rx
            .try_recv()
            .expect("a captured key must still reach the router so its action can fire");
        assert_eq!(routed.physical_code, "Numpad5");
    }

    #[test]
    fn tap_thread_reports_startup_failure_over_the_handshake() {
        let shared = Arc::new(TapShared::new());
        let status_shared = Arc::clone(&shared);
        let (startup_tx, startup_rx) = std::sync::mpsc::channel();

        let handle = std::thread::spawn(move || {
            run_tap_thread(shared, startup_tx, || Err(()));
        });
        let status = startup_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("startup handshake must fire");
        handle.join().expect("tap thread joins");

        assert!(matches!(status, StartupStatus::Failed(_)));
        assert_eq!(*lock(&status_shared.status), TapStatus::StartFailed);
    }
}
