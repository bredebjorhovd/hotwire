//! Capture policy: which physical keys Hotwire consumes, and when.

use std::collections::BTreeSet;

use hotwire_core::PhysicalKeyEvent;

use crate::bypass::EmergencyBypass;

/// How aggressively a profile consumes matching keys.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureMode {
    /// Assigned keys are consumed and never reach other applications.
    Capture,
    /// Keys are observed and routed but never consumed.
    Passthrough,
}

/// The set of physical codes Hotwire is allowed to consume.
///
/// Pure and platform-neutral: given a [`CaptureMode`] and the bound keys it
/// decides whether a normalized event should be suppressed. It never touches
/// the OS and never suppresses Hotwire-injected events (injection-loop
/// prevention) or unbound keys (fail-open passthrough).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapturePolicy {
    mode: CaptureMode,
    captured_keys: BTreeSet<String>,
}

impl CapturePolicy {
    /// Creates a policy for `mode` over `captured_keys`.
    #[must_use]
    pub fn new(mode: CaptureMode, captured_keys: impl IntoIterator<Item = String>) -> Self {
        Self {
            mode,
            captured_keys: captured_keys.into_iter().collect(),
        }
    }

    /// Returns the current [`CaptureMode`].
    #[must_use]
    pub fn mode(&self) -> CaptureMode {
        self.mode
    }

    /// Sets the capture mode.
    pub fn set_mode(&mut self, mode: CaptureMode) {
        self.mode = mode;
    }

    /// Returns the captured physical codes.
    #[must_use]
    pub fn captured_keys(&self) -> &BTreeSet<String> {
        &self.captured_keys
    }

    /// Replaces the captured physical codes.
    pub fn set_captured_keys(&mut self, keys: impl IntoIterator<Item = String>) {
        self.captured_keys = keys.into_iter().collect();
    }

    /// Returns whether `event` should be suppressed under this policy alone.
    ///
    /// Suppression requires capture mode, a non-injected event (Hotwire's own
    /// injections must always pass through), and a bound physical code. Repeats
    /// of a held bound key are suppressed so a held capture never leaks into
    /// the active application.
    #[must_use]
    pub fn should_suppress(&self, event: &PhysicalKeyEvent) -> bool {
        self.mode == CaptureMode::Capture
            && !event.is_injected
            && self.captured_keys.contains(&event.physical_code)
    }
}

/// What the capture gate decided for one normalized event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GateDecision {
    /// The event was consumed by Hotwire and will not reach other apps.
    pub suppressed: bool,
    /// Capture is currently paused by the emergency bypass.
    pub paused: bool,
    /// This exact event is the emergency-bypass chord press and must never be
    /// routed as a binding (the chord cannot be remapped).
    pub bypass_chord: bool,
}

/// Combines the [`CapturePolicy`] with the [`EmergencyBypass`] into the single
/// decision the native callback needs.
///
/// The emergency bypass is consulted first so it keeps working while capture
/// is paused, and a paused gate never suppresses anything (fail-open).
#[derive(Debug)]
pub struct CaptureGate {
    policy: CapturePolicy,
    bypass: EmergencyBypass,
}

impl CaptureGate {
    /// Creates a gate with default settings: capture mode, no captured keys,
    /// and the default emergency bypass chord (`Control` + `Option` + `Command`
    /// + `Escape`).
    #[must_use]
    pub fn new() -> Self {
        Self {
            policy: CapturePolicy::new(CaptureMode::Capture, []),
            bypass: EmergencyBypass::new(),
        }
    }

    /// Returns the underlying capture policy.
    #[must_use]
    pub fn policy(&self) -> &CapturePolicy {
        &self.policy
    }

    /// Mutably returns the underlying capture policy.
    pub fn policy_mut(&mut self) -> &mut CapturePolicy {
        &mut self.policy
    }

    /// Returns whether capture is paused by the emergency bypass.
    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.bypass.is_paused()
    }

    /// Pauses capture (fail-open) as if the emergency bypass was pressed.
    pub fn emergency_pause(&mut self) {
        self.bypass.pause();
    }

    /// Resumes capture after an emergency pause.
    pub fn emergency_resume(&mut self) {
        self.bypass.resume();
    }

    /// Feeds one normalized event through the bypass and produces the decision.
    ///
    /// Must be called from the input callback thread; it is pure and fast and
    /// must not execute actions.
    #[must_use]
    pub fn decide(&mut self, event: &PhysicalKeyEvent) -> GateDecision {
        let bypass_chord = self.bypass.on_event(event).is_some();
        GateDecision {
            suppressed: !self.bypass.is_paused() && self.policy.should_suppress(event),
            paused: self.bypass.is_paused(),
            bypass_chord,
        }
    }
}

impl Default for CaptureGate {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotwire_core::{KeyState, ModifierState};

    fn event(code: &str, is_injected: bool, is_repeat: bool) -> PhysicalKeyEvent {
        PhysicalKeyEvent {
            device_hint: None,
            scan_code: 0x57,
            physical_code: code.into(),
            state: KeyState::Down,
            modifiers: ModifierState::default(),
            timestamp_ns: 1,
            is_repeat,
            is_injected,
        }
    }

    #[test]
    fn capture_consumes_matched_non_injected_keys() {
        let policy = CapturePolicy::new(CaptureMode::Capture, ["Numpad5".into()]);
        assert!(policy.should_suppress(&event("Numpad5", false, false)));
    }

    #[test]
    fn capture_never_consumes_injected_events() {
        let policy = CapturePolicy::new(CaptureMode::Capture, ["Numpad5".into()]);
        assert!(!policy.should_suppress(&event("Numpad5", true, false)));
    }

    #[test]
    fn capture_never_consumes_unmatched_keys() {
        let policy = CapturePolicy::new(CaptureMode::Capture, ["Numpad5".into()]);
        assert!(!policy.should_suppress(&event("A", false, false)));
    }

    #[test]
    fn passthrough_never_consumes_anything() {
        let policy = CapturePolicy::new(CaptureMode::Passthrough, ["Numpad5".into()]);
        assert!(!policy.should_suppress(&event("Numpad5", false, false)));
    }

    #[test]
    fn held_repeat_and_up_of_a_captured_key_are_suppressed() {
        let policy = CapturePolicy::new(CaptureMode::Capture, ["Numpad0".into()]);
        assert!(policy.should_suppress(&event("Numpad0", false, true)));
        let mut up = event("Numpad0", false, false);
        up.state = KeyState::Up;
        assert!(policy.should_suppress(&up));
    }

    #[test]
    fn setting_the_mode_and_keys_updates_decisions() {
        let mut policy = CapturePolicy::new(CaptureMode::Passthrough, ["Numpad5".into()]);
        policy.set_mode(CaptureMode::Capture);
        assert!(policy.should_suppress(&event("Numpad5", false, false)));
        policy.set_captured_keys(["Numpad0".into()]);
        assert!(!policy.should_suppress(&event("Numpad5", false, false)));
        assert!(policy.should_suppress(&event("Numpad0", false, false)));
    }

    #[test]
    fn gate_suppresses_only_when_capture_is_active_and_unpaused() {
        let mut gate = CaptureGate::new();
        gate.policy_mut().set_captured_keys(["Numpad5".into()]);

        let mut escape = event("Escape", false, false);
        escape.modifiers.control = true;
        escape.modifiers.option = true;
        escape.modifiers.command = true;

        assert!(gate.decide(&event("Numpad5", false, false)).suppressed);
        assert_eq!(
            gate.decide(&escape),
            GateDecision {
                suppressed: false,
                paused: true,
                bypass_chord: true,
            }
        );
        assert!(gate.is_paused());
        assert!(!gate.decide(&event("Numpad5", false, false)).suppressed);

        assert_eq!(
            gate.decide(&escape),
            GateDecision {
                suppressed: false,
                paused: false,
                bypass_chord: true,
            }
        );
        assert!(gate.decide(&event("Numpad5", false, false)).suppressed);
    }
}
