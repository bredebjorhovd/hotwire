//! Raw-event diagnostics: an explicitly opted-in, auto-expiring surface.
//!
//! The persistent log ([`crate::SafetyLog`]) never records raw input. When a
//! user explicitly opts in for debugging (spec §15.1), raw normalized events
//! are collected here in memory for a short window and then dropped — this is
//! a separate surface, never persisted, and it disables itself as soon as the
//! window lapses or the app stops the session. It is off by default.

use std::time::{Duration, Instant};

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
/// Off by default. `enable` must be called explicitly with a short window;
/// the surface auto-disables and drops its samples when the window lapses.
/// Samples live in memory only and are never written to the persistent log.
#[derive(Debug, Default)]
pub struct RawEventDiagnostics {
    enabled: bool,
    window: Duration,
    enabled_at: Option<Instant>,
    samples: Vec<RawEventSample>,
}

impl RawEventDiagnostics {
    /// Creates a disabled buffer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Explicitly opts in for `window`, replacing any previous session.
    pub fn enable(&mut self, window: Duration) {
        self.enabled = true;
        self.window = window;
        self.enabled_at = Some(Instant::now());
        self.samples.clear();
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
    pub fn record(&mut self, sample: RawEventSample) {
        self.prune();
        if self.enabled {
            self.samples.push(sample);
        }
    }

    /// Returns a copy of the current samples, pruning any expired session.
    #[must_use]
    pub fn samples(&mut self) -> Vec<RawEventSample> {
        self.prune();
        self.samples.clone()
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

    fn sample() -> RawEventSample {
        RawEventSample::new(82, "Numpad0", true, 1, false, false)
    }

    #[test]
    fn raw_event_diagnostics_are_off_by_default_and_never_record() {
        let mut diag = RawEventDiagnostics::new();
        assert!(!diag.is_enabled());
        diag.record(sample());
        assert!(diag.samples().is_empty());
    }

    #[test]
    fn opt_in_captures_until_the_window_auto_expires() {
        let mut diag = RawEventDiagnostics::new();
        diag.enable(Duration::from_millis(50));
        assert!(diag.is_enabled());
        diag.record(sample());
        assert_eq!(diag.samples().len(), 1);

        std::thread::sleep(Duration::from_millis(80));
        assert!(!diag.is_enabled(), "the opt-in surface must auto-expire");
        assert!(diag.samples().is_empty(), "expired samples are dropped");
    }

    #[test]
    fn explicit_disable_stops_capture_immediately() {
        let mut diag = RawEventDiagnostics::new();
        diag.enable(Duration::from_secs(10));
        diag.record(sample());
        diag.disable();
        assert!(!diag.is_enabled());
        diag.record(sample());
        assert!(diag.samples().is_empty());
    }
}
