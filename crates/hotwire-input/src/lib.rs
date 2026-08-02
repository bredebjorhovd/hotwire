//! Platform-neutral input boundary.
//!
//! Native input backends normalize raw OS events into
//! [`hotwire_core::PhysicalKeyEvent`] values and hand them over on a channel.
//! This crate owns the *trigger detection* logic, the *capture policy* and
//! *emergency bypass* that decide which keys Hotwire consumes, and the
//! *backend seam* that the macOS and Windows implementations fill in. It never
//! touches the OS and never executes actions.

mod bypass;
mod capture;

use std::sync::mpsc::Sender;

use hotwire_core::{KeyState, PhysicalKeyEvent};
use thiserror::Error;

pub use bypass::{BypassAction, EmergencyBypass, ModifierChord};
pub use capture::{CaptureGate, CaptureMode, CapturePolicy, GateDecision};
pub use hotwire_core::{ModifierState, Trigger};

/// Error surfaced by an [`InputBackend`] when it cannot start or is not yet
/// implemented on the current platform.
#[derive(Debug, Error)]
pub enum BackendError {
    /// The backend exists as a seam but has no implementation on this platform.
    #[error("input backend `{0}` is not implemented yet")]
    NotImplemented(&'static str),
    /// The backend failed to acquire OS-level input capture.
    #[error("input backend failed to start: {0}")]
    Start(String),
}

/// The platform seam for raw input capture.
///
/// Implementations (macOS Quartz event tap, Windows `WH_KEYBOARD_LL`) must
/// normalize and enqueue only. They must never execute an action themselves.
pub trait InputBackend: Send + Sync {
    /// Stable backend identifier, e.g. `"macos-quartz"`.
    fn name(&self) -> &'static str;

    /// Begin delivering normalized events to `sink`.
    ///
    /// # Errors
    ///
    /// Returns [`BackendError::NotImplemented`] when the platform backend is a
    /// placeholder, or [`BackendError::Start`] when OS capture is unavailable
    /// (for example, missing Accessibility/Input Monitoring permission).
    fn start(&self, sink: Sender<PhysicalKeyEvent>) -> Result<(), BackendError>;

    /// Best-effort stop of the backend.
    ///
    /// The default does nothing. Backends that spawn resources must release
    /// them here and must not leave a logical key held down (see the
    /// fail-open invariant).
    fn stop(&self) {}
}

/// One step of a detected interaction, ordered per physical key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TriggerEvent {
    /// The action should begin.
    Down(Trigger),
    /// The action should end (key released).
    Up(Trigger),
    /// The interaction was abandoned without firing.
    Cancelled(Trigger),
}

/// Detects a single binding's [`Trigger`] from a normalized key-event stream.
///
/// The detector is pure: it takes events and optionally explicit time ticks,
/// and returns any [`TriggerEvent`]s that resulted. Time is expressed as
/// nanoseconds matching [`PhysicalKeyEvent::timestamp_ns`].
#[derive(Debug)]
pub struct TriggerDetector {
    trigger: Trigger,
    double_press_window_ns: u64,
    phase: DetectorPhase,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum DetectorPhase {
    Idle,
    /// A press or hold is in flight, or a double-press first press was armed.
    Down {
        at_ns: u64,
    },
    /// The first press of a double press was released; waiting for the second.
    WaitSecond {
        first_down_at_ns: u64,
    },
    /// A double press fired and is awaiting release.
    DoublePressed {
        at_ns: u64,
    },
}

impl TriggerDetector {
    /// Creates a detector for `trigger`.
    ///
    /// `double_press_window_ns` only affects `double_press` triggers and is
    /// ignored for `press` and `hold`.
    #[must_use]
    pub fn new(trigger: Trigger, double_press_window_ns: u64) -> Self {
        Self {
            trigger,
            double_press_window_ns,
            phase: DetectorPhase::Idle,
        }
    }

    /// Feeds a normalized event and returns any resulting [`TriggerEvent`]s.
    #[must_use]
    pub fn on_event(&mut self, event: &PhysicalKeyEvent) -> Vec<TriggerEvent> {
        if event.is_injected {
            return Vec::new();
        }
        match event.state {
            KeyState::Down => self.on_down(event.timestamp_ns),
            KeyState::Up => self.on_up(),
        }
    }

    /// Advances the detector to `now_ns`, expiring stale double-press waits.
    #[must_use]
    pub fn on_tick(&mut self, now_ns: u64) -> Vec<TriggerEvent> {
        let DetectorPhase::WaitSecond { first_down_at_ns } = self.phase else {
            return Vec::new();
        };
        if now_ns.saturating_sub(first_down_at_ns) > self.double_press_window_ns {
            self.phase = DetectorPhase::Idle;
            return vec![TriggerEvent::Cancelled(Trigger::DoublePress)];
        }
        Vec::new()
    }

    fn on_down(&mut self, at_ns: u64) -> Vec<TriggerEvent> {
        match (self.trigger, self.phase) {
            (Trigger::Press | Trigger::Hold, DetectorPhase::Idle) => {
                self.phase = DetectorPhase::Down { at_ns };
                vec![TriggerEvent::Down(self.trigger)]
            }
            (Trigger::DoublePress, DetectorPhase::Idle) => {
                self.phase = DetectorPhase::Down { at_ns };
                Vec::new()
            }
            (Trigger::DoublePress, DetectorPhase::WaitSecond { first_down_at_ns }) => {
                let within_window =
                    at_ns.saturating_sub(first_down_at_ns) <= self.double_press_window_ns;
                self.phase = if within_window {
                    DetectorPhase::DoublePressed { at_ns }
                } else {
                    DetectorPhase::Down { at_ns }
                };
                if within_window {
                    vec![TriggerEvent::Down(Trigger::DoublePress)]
                } else {
                    Vec::new()
                }
            }
            (Trigger::Press | Trigger::Hold, DetectorPhase::Down { .. })
            | (
                Trigger::DoublePress,
                DetectorPhase::Down { .. } | DetectorPhase::DoublePressed { .. },
            ) => Vec::new(),
            (
                Trigger::Press | Trigger::Hold,
                DetectorPhase::WaitSecond { .. } | DetectorPhase::DoublePressed { .. },
            ) => unreachable!(),
        }
    }

    fn on_up(&mut self) -> Vec<TriggerEvent> {
        match (self.trigger, self.phase) {
            (Trigger::Press | Trigger::Hold, DetectorPhase::Down { .. }) => {
                self.phase = DetectorPhase::Idle;
                vec![TriggerEvent::Up(self.trigger)]
            }
            (Trigger::DoublePress, DetectorPhase::Down { at_ns }) => {
                self.phase = DetectorPhase::WaitSecond {
                    first_down_at_ns: at_ns,
                };
                Vec::new()
            }
            (Trigger::DoublePress, DetectorPhase::DoublePressed { .. }) => {
                self.phase = DetectorPhase::Idle;
                vec![TriggerEvent::Up(Trigger::DoublePress)]
            }
            (Trigger::DoublePress, DetectorPhase::WaitSecond { .. } | DetectorPhase::Idle)
            | (Trigger::Press | Trigger::Hold, DetectorPhase::Idle) => Vec::new(),
            (
                Trigger::Press | Trigger::Hold,
                DetectorPhase::WaitSecond { .. } | DetectorPhase::DoublePressed { .. },
            ) => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotwire_core::{ModifierState, PhysicalKeyEvent};

    const DOUBLE_PRESS_WINDOW_NS: u64 = 250_000_000;

    fn event(timestamp_ns: u64, state: KeyState) -> PhysicalKeyEvent {
        PhysicalKeyEvent {
            device_hint: None,
            scan_code: 82,
            physical_code: "Numpad0".into(),
            state,
            modifiers: ModifierState::default(),
            timestamp_ns,
            is_repeat: false,
            is_injected: false,
        }
    }

    #[test]
    fn press_fires_once_on_down_and_ends_on_up() {
        let mut detector = TriggerDetector::new(Trigger::Press, DOUBLE_PRESS_WINDOW_NS);

        let first_down = detector.on_event(&event(0, KeyState::Down));
        assert_eq!(first_down, vec![TriggerEvent::Down(Trigger::Press)]);

        let repeat = detector.on_event(&event(100, KeyState::Down));
        assert!(repeat.is_empty());

        let up = detector.on_event(&event(200, KeyState::Up));
        assert_eq!(up, vec![TriggerEvent::Up(Trigger::Press)]);
    }

    #[test]
    fn hold_starts_on_down_ends_on_up_and_ignores_repeats() {
        let mut detector = TriggerDetector::new(Trigger::Hold, DOUBLE_PRESS_WINDOW_NS);

        assert_eq!(
            detector.on_event(&event(0, KeyState::Down)),
            vec![TriggerEvent::Down(Trigger::Hold)]
        );
        assert!(detector.on_event(&event(50, KeyState::Down)).is_empty());
        assert_eq!(
            detector.on_event(&event(1_000_000, KeyState::Up)),
            vec![TriggerEvent::Up(Trigger::Hold)]
        );
    }

    #[test]
    fn double_press_fires_only_on_second_down_inside_window() {
        let mut detector = TriggerDetector::new(Trigger::DoublePress, DOUBLE_PRESS_WINDOW_NS);

        assert!(detector.on_event(&event(0, KeyState::Down)).is_empty());
        assert!(detector.on_event(&event(50, KeyState::Up)).is_empty());
        assert_eq!(
            detector.on_event(&event(150, KeyState::Down)),
            vec![TriggerEvent::Down(Trigger::DoublePress)]
        );
        assert_eq!(
            detector.on_event(&event(200, KeyState::Up)),
            vec![TriggerEvent::Up(Trigger::DoublePress)]
        );
    }

    #[test]
    fn double_press_expires_when_window_lapses() {
        let mut detector = TriggerDetector::new(Trigger::DoublePress, DOUBLE_PRESS_WINDOW_NS);

        assert!(detector.on_event(&event(0, KeyState::Down)).is_empty());
        assert!(detector.on_event(&event(50, KeyState::Up)).is_empty());
        assert_eq!(
            detector.on_tick(DOUBLE_PRESS_WINDOW_NS + 1),
            vec![TriggerEvent::Cancelled(Trigger::DoublePress)]
        );
        assert!(detector
            .on_event(&event(1_000_000, KeyState::Down))
            .is_empty());
    }

    #[test]
    fn injected_events_are_never_detected() {
        let mut detector = TriggerDetector::new(Trigger::Press, DOUBLE_PRESS_WINDOW_NS);
        let mut injected = event(0, KeyState::Down);
        injected.is_injected = true;

        assert!(detector.on_event(&injected).is_empty());
    }
}
