//! Raw-event diagnostics: an explicitly opted-in, auto-expiring surface.
//!
//! The persistent log ([`crate::SafetyLog`]) never records raw input. When a
//! user explicitly opts in for debugging (spec §15.1), raw normalized events
//! are collected here in memory for a short, bounded window and then dropped —
//! this is a separate surface, never persisted, and it disables itself as soon
//! as the window lapses or the app stops the session. It is off by default,
//! the window is capped at a short maximum, and the sample buffer is bounded
//! (oldest samples are dropped on overflow).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use thiserror::Error;

/// The longest a raw-event session may run before auto-disabling (§15.1).
pub const MAX_RAW_WINDOW: Duration = Duration::from_secs(60);

/// The maximum number of samples retained during a session (bounded buffer).
pub const MAX_RAW_SAMPLES: usize = 1024;

/// Errors produced while starting a raw-event session.
#[derive(Debug, Error)]
pub enum RawEventError {
    /// A zero-length window would auto-expire immediately.
    #[error("the raw-event window must be greater than zero")]
    EmptyWindow,
    /// The requested window exceeds the maximum (§15.1 keeps it short).
    #[error("the raw-event window {0:?} exceeds the maximum of {MAX_RAW_WINDOW:?}")]
    WindowTooLong(Duration),
}

/// One normalized input event captured while raw-event diagnostics are enabled.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawEventSample {
    /// The platform scan code.
    pub scan_code: u32,
    /// The normalized physical code (a bound key is a single configured code).
    pub physical_code: String,
    /// Whether the event was a key-down (`true`) or key-up (`false`).
    pub is_down: bool,
    /// Event timestamp in nanoseconds.
    pub timestamp_ns: u64,
    /// Whether the OS flagged the event as an auto-repeat.
    pub is_repeat: bool,
    /// Whether the event was injected by Hotwire itself.
    pub is_injected: bool,
}

impl RawEventSample {
    /// Creates a sample.
    #[must_use]
    pub fn new(
        scan_code: u32,
        physical_code: impl Into<String>,
        is_down: bool,
        timestamp_ns: u64,
        is_repeat: bool,
        is_injected: bool,
    ) -> Self {
        Self {
            scan_code,
            physical_code: physical_code.into(),
            is_down,
            timestamp_ns,
            is_repeat,
            is_injected,
        }
    }
}

/// The opt-in raw-event capture buffer (spec §15.1).
///
/// Off by default. `enable` must be called explicitly with a short window that
/// is capped at [`MAX_RAW_WINDOW`]; the surface auto-disables and drops its
/// samples when the window lapses, and keeps at most [`MAX_RAW_SAMPLES`]
/// samples. Samples live in memory only and are never written to the
/// persistent log.
#[derive(Debug, Default)]
pub struct RawEventDiagnostics {
    enabled: bool,
    window: Duration,
    enabled_at: Option<Instant>,
    samples: VecDeque<RawEventSample>,
}

impl RawEventDiagnostics {
    /// Creates a disabled buffer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Explicitly opts in for `window`, replacing any previous session.
    ///
    /// The window must be greater than zero and at most [`MAX_RAW_WINDOW`];
    /// anything longer is rejected rather than honored.
    ///
    /// # Errors
    ///
    /// Returns [`RawEventError::EmptyWindow`] for a zero window and
    /// [`RawEventError::WindowTooLong`] for a window beyond the maximum.
    pub fn enable(&mut self, window: Duration) -> Result<(), RawEventError> {
        if window.is_zero() {
            return Err(RawEventError::EmptyWindow);
        }
        if window > MAX_RAW_WINDOW {
            return Err(RawEventError::WindowTooLong(window));
        }
        self.enabled = true;
        self.window = window;
        self.enabled_at = Some(Instant::now());
        self.samples.clear();
        Ok(())
    }

    /// Explicitly stops the session and drops its samples.
    pub fn disable(&mut self) {
        self.enabled = false;
        self.enabled_at = None;
        self.samples.clear();
    }

    /// Returns whether a session is currently active (not auto-expired).
    #[must_use]
    pub fn is_enabled(&mut self) -> bool {
        self.prune();
        self.enabled
    }

    /// Records a raw event sample, unless the session is inactive or expired.
    ///
    /// When the bounded buffer is full, the oldest sample is dropped so the
    /// buffer never grows beyond [`MAX_RAW_SAMPLES`].
    pub fn record(&mut self, sample: RawEventSample) {
        self.prune();
        if self.enabled {
            if self.samples.len() >= MAX_RAW_SAMPLES {
                self.samples.pop_front();
            }
            self.samples.push_back(sample);
        }
    }

    /// Returns a copy of the current samples, pruning any expired session.
    #[must_use]
    pub fn samples(&mut self) -> Vec<RawEventSample> {
        self.prune();
        self.samples.iter().cloned().collect()
    }

    /// Drops all samples while keeping the session active.
    pub fn clear(&mut self) {
        self.samples.clear();
    }

    fn prune(&mut self) {
        if let Some(started) = self.enabled_at {
            if started.elapsed() >= self.window {
                self.enabled = false;
                self.enabled_at = None;
                self.samples.clear();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(scan_code: u32) -> RawEventSample {
        RawEventSample::new(scan_code, "Numpad0", true, 1, false, false)
    }

    #[test]
    fn raw_event_diagnostics_are_off_by_default_and_never_record() {
        let mut diag = RawEventDiagnostics::new();
        assert!(!diag.is_enabled());
        diag.record(sample(1));
        assert!(diag.samples().is_empty());
    }

    #[test]
    fn opt_in_captures_until_the_window_auto_expires() {
        let mut diag = RawEventDiagnostics::new();
        diag.enable(Duration::from_millis(50)).expect("enable");
        assert!(diag.is_enabled());
        diag.record(sample(1));
        assert_eq!(diag.samples().len(), 1);

        std::thread::sleep(Duration::from_millis(80));
        assert!(!diag.is_enabled(), "the opt-in surface must auto-expire");
        assert!(diag.samples().is_empty(), "expired samples are dropped");
    }

    #[test]
    fn excessive_or_empty_windows_are_rejected() {
        let mut diag = RawEventDiagnostics::new();
        assert!(matches!(
            diag.enable(Duration::ZERO),
            Err(RawEventError::EmptyWindow)
        ));
        assert!(matches!(
            diag.enable(Duration::from_secs(3600)),
            Err(RawEventError::WindowTooLong(_))
        ));
        assert!(!diag.is_enabled());

        diag.enable(Duration::from_secs(10))
            .expect("a short window is allowed");
        assert!(diag.is_enabled());
    }

    #[test]
    fn the_sample_buffer_is_bounded_and_drops_the_oldest_on_overflow() {
        let mut diag = RawEventDiagnostics::new();
        diag.enable(Duration::from_secs(30)).expect("enable");

        for i in 0..(MAX_RAW_SAMPLES + 50) {
            diag.record(sample(u32::try_from(i).unwrap_or(u32::MAX)));
        }
        let samples = diag.samples();
        assert_eq!(samples.len(), MAX_RAW_SAMPLES);
        assert_eq!(
            samples[0].scan_code,
            u32::try_from(50).expect("fits"),
            "the oldest samples must be dropped first"
        );
        assert_eq!(
            samples[MAX_RAW_SAMPLES - 1].scan_code,
            u32::try_from(MAX_RAW_SAMPLES + 49).expect("fits"),
            "the newest samples must be retained"
        );
    }

    #[test]
    fn explicit_disable_stops_capture_immediately() {
        let mut diag = RawEventDiagnostics::new();
        diag.enable(Duration::from_secs(10)).expect("enable");
        diag.record(sample(1));
        diag.disable();
        assert!(!diag.is_enabled());
        diag.record(sample(2));
        assert!(diag.samples().is_empty());
    }
}
