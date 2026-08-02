//! Normalization of raw Quartz keyboard events into core types.
//!
//! This is the only place a `CGEvent` is read. It is pure and allocation-light
//! so it can run inside the event-tap callback; the callback enqueues the
//! result and never does anything else.
//!
//! Quartz integer fields are documented signed values that hold keycodes and
//! flags; the casts below reinterpret those known-small, non-negative values.

#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use core_graphics::event::{CGEvent, CGEventFlags, CGEventType, EventField, KeyCode};
use hotwire_core::{KeyState, ModifierState, PhysicalKeyEvent};

use crate::ffi;
use crate::keycode;
use crate::INJECTED_MARKER;

/// Normalizes a keyboard event, returning `None` for event types outside the
/// tap's interest mask.
#[must_use]
pub fn normalize_event(event: &CGEvent) -> Option<PhysicalKeyEvent> {
    match event.get_type() {
        CGEventType::KeyDown => Some(normalize_key(event, KeyState::Down)),
        CGEventType::KeyUp => Some(normalize_key(event, KeyState::Up)),
        CGEventType::FlagsChanged => normalize_flags_changed(event),
        _ => None,
    }
}

fn normalize_key(event: &CGEvent, state: KeyState) -> PhysicalKeyEvent {
    let scan_code = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u32;
    PhysicalKeyEvent {
        device_hint: None,
        scan_code,
        physical_code: keycode::physical_name(scan_code as u16),
        state,
        modifiers: modifier_state(event.get_flags()),
        timestamp_ns: ffi::event_timestamp_ns(event),
        is_repeat: event.get_integer_value_field(EventField::KEYBOARD_EVENT_AUTOREPEAT) != 0,
        is_injected: is_injected(event),
    }
}

/// Normalizes a modifier-only `FlagsChanged` event into a key event for the
/// modifier key that changed, deriving the state from the event flags.
fn normalize_flags_changed(event: &CGEvent) -> Option<PhysicalKeyEvent> {
    let scan_code = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u32;
    let flags = event.get_flags();
    let (physical_code, down) = match scan_code as u16 {
        KeyCode::SHIFT | KeyCode::RIGHT_SHIFT => {
            ("Shift", flags.contains(CGEventFlags::CGEventFlagShift))
        }
        KeyCode::CONTROL | KeyCode::RIGHT_CONTROL => {
            ("Control", flags.contains(CGEventFlags::CGEventFlagControl))
        }
        KeyCode::OPTION | KeyCode::RIGHT_OPTION => {
            ("Option", flags.contains(CGEventFlags::CGEventFlagAlternate))
        }
        KeyCode::COMMAND | KeyCode::RIGHT_COMMAND => {
            ("Command", flags.contains(CGEventFlags::CGEventFlagCommand))
        }
        _ => return None,
    };
    Some(PhysicalKeyEvent {
        device_hint: None,
        scan_code,
        physical_code: physical_code.into(),
        state: if down { KeyState::Down } else { KeyState::Up },
        modifiers: modifier_state(flags),
        timestamp_ns: ffi::event_timestamp_ns(event),
        is_repeat: false,
        is_injected: is_injected(event),
    })
}

/// Returns whether the event carries Hotwire's injection signature.
#[must_use]
pub fn is_injected(event: &CGEvent) -> bool {
    event.get_integer_value_field(EventField::EVENT_SOURCE_USER_DATA) as u64 == INJECTED_MARKER
}

/// Maps Quartz event flags onto Hotwire's modifier state.
#[must_use]
pub fn modifier_state(flags: CGEventFlags) -> ModifierState {
    ModifierState {
        shift: flags.contains(CGEventFlags::CGEventFlagShift),
        control: flags.contains(CGEventFlags::CGEventFlagControl),
        option: flags.contains(CGEventFlags::CGEventFlagAlternate),
        command: flags.contains(CGEventFlags::CGEventFlagCommand),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    fn key_event(keycode: u16, down: bool) -> CGEvent {
        let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
            .expect("event source creation should succeed");
        CGEvent::new_keyboard_event(source, keycode, down)
            .expect("keyboard event creation should succeed")
    }

    #[test]
    fn key_down_normalizes_to_down_state() {
        let event = key_event(KeyCode::ANSI_KEYPAD_5, true);
        let normalized = normalize_event(&event).expect("key event normalizes");

        assert_eq!(normalized.state, KeyState::Down);
        assert_eq!(normalized.physical_code, "Numpad5");
        assert_eq!(normalized.scan_code, u32::from(KeyCode::ANSI_KEYPAD_5));
        assert!(!normalized.is_repeat);
        assert!(!normalized.is_injected);
        assert_eq!(normalized.timestamp_ns, ffi::event_timestamp_ns(&event));
    }

    #[test]
    fn timestamps_are_passed_through_in_nanoseconds() {
        // `CGEventTimestamp` is already elapsed nanoseconds since startup; the
        // unit contract is that normalization copies it verbatim. Pinning an
        // exact value guards against ever re-scaling it (the former µs×1000
        // bug inflated every event delta by 1,000 and broke double-press
        // timing).
        let timestamp_ns = 12_345_678_901_234;
        let event = key_event(KeyCode::ANSI_KEYPAD_5, true);
        ffi::set_event_timestamp(&event, timestamp_ns);

        let normalized = normalize_event(&event).expect("key event normalizes");
        assert_eq!(normalized.timestamp_ns, timestamp_ns);
    }

    #[test]
    fn key_up_normalizes_to_up_state() {
        let event = key_event(KeyCode::ANSI_KEYPAD_0, false);
        let normalized = normalize_event(&event).expect("key event normalizes");

        assert_eq!(normalized.state, KeyState::Up);
        assert_eq!(normalized.physical_code, "Numpad0");
    }

    #[test]
    fn autorepeat_is_flagged() {
        let event = key_event(KeyCode::ANSI_KEYPAD_5, true);
        event.set_integer_value_field(EventField::KEYBOARD_EVENT_AUTOREPEAT, 1);

        let normalized = normalize_event(&event).expect("key event normalizes");
        assert!(normalized.is_repeat);
    }

    #[test]
    fn injected_signature_is_detected() {
        let event = key_event(KeyCode::ANSI_KEYPAD_5, true);
        event.set_integer_value_field(
            EventField::EVENT_SOURCE_USER_DATA,
            i64::try_from(INJECTED_MARKER).expect("marker fits in i64"),
        );

        let normalized = normalize_event(&event).expect("key event normalizes");
        assert!(normalized.is_injected);
    }

    #[test]
    fn modifiers_are_captured_from_event_flags() {
        let event = key_event(KeyCode::ANSI_KEYPAD_5, true);
        event.set_flags(CGEventFlags::CGEventFlagShift | CGEventFlags::CGEventFlagCommand);

        let normalized = normalize_event(&event).expect("key event normalizes");
        assert!(normalized.modifiers.shift);
        assert!(normalized.modifiers.command);
        assert!(!normalized.modifiers.control);
        assert!(!normalized.modifiers.option);
    }

    #[test]
    fn unknown_keycodes_keep_a_stable_label() {
        let event = key_event(0xF4, true);
        let normalized = normalize_event(&event).expect("key event normalizes");

        assert_eq!(normalized.physical_code, "Unknown(0xf4)");
        assert_eq!(normalized.scan_code, 0xF4);
    }

    #[test]
    fn flags_changed_maps_modifier_keys_to_down_and_up() {
        let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
            .expect("event source creation should succeed");

        let down = CGEvent::new_keyboard_event(source.clone(), KeyCode::SHIFT, false)
            .expect("flags event creation should succeed");
        down.set_type(CGEventType::FlagsChanged);
        down.set_flags(CGEventFlags::CGEventFlagShift);

        let normalized = normalize_event(&down).expect("flags event normalizes");
        assert_eq!(normalized.physical_code, "Shift");
        assert_eq!(normalized.state, KeyState::Down);
        assert!(normalized.modifiers.shift);

        let up = CGEvent::new_keyboard_event(source, KeyCode::SHIFT, false)
            .expect("flags event creation should succeed");
        up.set_type(CGEventType::FlagsChanged);
        up.set_flags(CGEventFlags::default());

        let normalized = normalize_event(&up).expect("flags event normalizes");
        assert_eq!(normalized.state, KeyState::Up);
        assert!(!normalized.modifiers.shift);
    }
}
