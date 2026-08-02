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
use core_graphics::event::{
    CGEvent, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
    CallbackResult,
};
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
fn tap_callback(shared: &TapShared, etype: CGEventType, event: &CGEvent) -> CallbackResult {
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
    if let Some(sink) = lock(&shared.sink).as_ref() {
        let _ = sink.send(normalized.clone());
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
#[allow(clippy::needless_pass_by_value)] // Arc ownership must transfer into the spawned thread.
pub(crate) fn run_tap_thread(shared: Arc<TapShared>) {
    let callback_shared = Arc::clone(&shared);
    let Ok(tap) = CGEventTap::new(
        CGEventTapLocation::HID,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::Default,
        vec![
            CGEventType::KeyDown,
            CGEventType::KeyUp,
            CGEventType::FlagsChanged,
        ],
        move |_proxy, etype, event| tap_callback(&callback_shared, etype, event),
    ) else {
        *lock(&shared.status) = TapStatus::StartFailed;
        return;
    };

    let Ok(source) = tap.mach_port().create_runloop_source(0) else {
        *lock(&shared.status) = TapStatus::StartFailed;
        return;
    };

    let runloop = CFRunLoop::get_current();
    runloop.add_source(&source, ffi::common_modes());
    tap.enable();
    *lock(&shared.status) = TapStatus::Running;
    *lock(&shared.runloop) = Some(runloop.clone());

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
