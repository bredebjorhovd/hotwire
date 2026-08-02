//! The emergency bypass: a Hotwire-unremappable chord that pauses capture.

use hotwire_core::{KeyState, ModifierState, PhysicalKeyEvent};

/// The modifier chord of the emergency bypass (`Control` + `Option` + `Command`,
/// Escape). Hotwire itself can never bind this chord.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)] // Independent OS modifier flags, not state.
pub struct ModifierChord {
    pub shift: bool,
    pub control: bool,
    pub option: bool,
    pub command: bool,
}

impl ModifierChord {
    /// The `Control` + `Option` + `Command` + `Escape` emergency chord.
    pub const EMERGENCY: Self = Self {
        shift: false,
        control: true,
        option: true,
        command: true,
    };

    /// Returns whether `event` carries exactly this chord's modifiers.
    #[must_use]
    pub fn matches(&self, modifiers: &ModifierState) -> bool {
        self.shift == modifiers.shift
            && self.control == modifiers.control
            && self.option == modifiers.option
            && self.command == modifiers.command
    }
}

/// What a bypass chord press did.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BypassAction {
    /// Capture was paused (fail-open).
    Paused,
    /// Capture was resumed.
    Resumed,
}

/// Detects the emergency bypass chord and remembers whether capture is paused.
///
/// The bypass is checked before the capture policy so it keeps working while
/// paused. It only fires on a fresh, non-injected key-down of the chord key
/// with the chord modifiers held; repeats and key-ups are ignored so one press
/// toggles exactly once.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmergencyBypass {
    chord: ModifierChord,
    physical_code: String,
    paused: bool,
}

impl EmergencyBypass {
    /// Creates a bypass for the default chord: `Control` + `Option` + `Command`
    /// + `Escape`, starting unpaused.
    #[must_use]
    pub fn new() -> Self {
        Self {
            chord: ModifierChord::EMERGENCY,
            physical_code: "Escape".into(),
            paused: false,
        }
    }

    /// Creates a bypass for a custom chord. The chord key is matched by
    /// physical code so it cannot be remapped by a profile.
    #[must_use]
    pub fn with_chord(chord: ModifierChord, physical_code: impl Into<String>) -> Self {
        Self {
            chord,
            physical_code: physical_code.into(),
            paused: false,
        }
    }

    /// Returns whether capture is currently paused.
    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Pauses capture (fail-open).
    pub fn pause(&mut self) {
        self.paused = true;
    }

    /// Resumes capture.
    pub fn resume(&mut self) {
        self.paused = false;
    }

    /// Feeds a normalized event and toggles the bypass when the chord fires.
    ///
    /// Returns the resulting [`BypassAction`] only on the exact press that
    /// toggled the bypass, and `None` otherwise.
    #[must_use]
    pub fn on_event(&mut self, event: &PhysicalKeyEvent) -> Option<BypassAction> {
        if event.is_injected
            || event.is_repeat
            || event.state != KeyState::Down
            || event.physical_code != self.physical_code
            || !self.chord.matches(&event.modifiers)
        {
            return None;
        }
        self.paused = !self.paused;
        Some(if self.paused {
            BypassAction::Paused
        } else {
            BypassAction::Resumed
        })
    }
}

impl Default for EmergencyBypass {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(physical_code: &str, modifiers: ModifierState, state: KeyState) -> PhysicalKeyEvent {
        PhysicalKeyEvent {
            device_hint: None,
            scan_code: 0x35,
            physical_code: physical_code.into(),
            state,
            modifiers,
            timestamp_ns: 1,
            is_repeat: false,
            is_injected: false,
        }
    }

    fn emergency_modifiers() -> ModifierState {
        ModifierState {
            shift: false,
            control: true,
            option: true,
            command: true,
        }
    }

    #[test]
    fn starts_unpaused() {
        assert!(!EmergencyBypass::new().is_paused());
    }

    #[test]
    fn chord_toggles_pause_and_resume() {
        let mut bypass = EmergencyBypass::new();
        let escape = event("Escape", emergency_modifiers(), KeyState::Down);

        assert_eq!(bypass.on_event(&escape), Some(BypassAction::Paused));
        assert!(bypass.is_paused());
        assert_eq!(bypass.on_event(&escape), Some(BypassAction::Resumed));
        assert!(!bypass.is_paused());
    }

    #[test]
    fn key_up_and_repeats_do_not_toggle() {
        let mut bypass = EmergencyBypass::new();
        let up = event("Escape", emergency_modifiers(), KeyState::Up);
        assert_eq!(bypass.on_event(&up), None);
        assert!(!bypass.is_paused());

        let mut repeat = event("Escape", emergency_modifiers(), KeyState::Down);
        repeat.is_repeat = true;
        assert_eq!(bypass.on_event(&repeat), None);
    }

    #[test]
    fn requires_the_chord_physical_key_and_all_modifiers() {
        let mut bypass = EmergencyBypass::new();
        let modifiers = emergency_modifiers();

        assert_eq!(
            bypass.on_event(&event("Escape", ModifierState::default(), KeyState::Down)),
            None
        );
        assert_eq!(
            bypass.on_event(&event("A", modifiers.clone(), KeyState::Down)),
            None
        );

        let mut missing_command = modifiers.clone();
        missing_command.command = false;
        assert_eq!(
            bypass.on_event(&event("Escape", missing_command, KeyState::Down)),
            None
        );

        let mut extra_shift = modifiers.clone();
        extra_shift.shift = true;
        assert_eq!(
            bypass.on_event(&event("Escape", extra_shift, KeyState::Down)),
            None
        );
    }

    #[test]
    fn injected_chord_never_toggles() {
        let mut bypass = EmergencyBypass::new();
        let mut escape = event("Escape", emergency_modifiers(), KeyState::Down);
        escape.is_injected = true;
        assert_eq!(bypass.on_event(&escape), None);
        assert!(!bypass.is_paused());
    }

    #[test]
    fn custom_chord_is_configurable() {
        let chord = ModifierChord {
            shift: false,
            control: true,
            option: false,
            command: true,
        };
        let mut bypass = EmergencyBypass::with_chord(chord, "NumpadDecimal");
        let press = event(
            "NumpadDecimal",
            ModifierState {
                shift: false,
                control: true,
                option: false,
                command: true,
            },
            KeyState::Down,
        );
        assert_eq!(bypass.on_event(&press), Some(BypassAction::Paused));
        assert!(bypass.is_paused());
    }
}
