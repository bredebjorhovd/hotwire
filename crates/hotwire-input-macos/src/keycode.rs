//! Virtual-keycode to canonical physical-code mapping.
//!
//! Hotwire operates on physical keys, not emitted characters (spec §9.4), so
//! the mapping is stable across Num Lock, keyboard layout, and the active
//! input method. On macOS the virtual keycode is the closest stable physical
//! identifier a Quartz event exposes, so it fills both `scan_code` and the
//! canonical `physical_code` name in the normalized event.

use core_graphics::event::{CGKeyCode, KeyCode};

/// Returns the canonical physical-code name for a virtual keycode, or a
/// `Unknown(0x..)` fallback so no key is ever silently dropped.
#[must_use]
pub fn physical_name(code: CGKeyCode) -> String {
    match code {
        // Numpad.
        KeyCode::ANSI_KEYPAD_0 => "Numpad0".into(),
        KeyCode::ANSI_KEYPAD_1 => "Numpad1".into(),
        KeyCode::ANSI_KEYPAD_2 => "Numpad2".into(),
        KeyCode::ANSI_KEYPAD_3 => "Numpad3".into(),
        KeyCode::ANSI_KEYPAD_4 => "Numpad4".into(),
        KeyCode::ANSI_KEYPAD_5 => "Numpad5".into(),
        KeyCode::ANSI_KEYPAD_6 => "Numpad6".into(),
        KeyCode::ANSI_KEYPAD_7 => "Numpad7".into(),
        KeyCode::ANSI_KEYPAD_8 => "Numpad8".into(),
        KeyCode::ANSI_KEYPAD_9 => "Numpad9".into(),
        KeyCode::ANSI_KEYPAD_PLUS => "NumpadAdd".into(),
        KeyCode::ANSI_KEYPAD_MINUS => "NumpadSubtract".into(),
        KeyCode::ANSI_KEYPAD_MULTIPLY => "NumpadMultiply".into(),
        KeyCode::ANSI_KEYPAD_DIVIDE => "NumpadDivide".into(),
        KeyCode::ANSI_KEYPAD_DECIMAL => "NumpadDecimal".into(),
        KeyCode::ANSI_KEYPAD_ENTER => "NumpadEnter".into(),
        KeyCode::ANSI_KEYPAD_CLEAR => "NumLock".into(),
        KeyCode::ANSI_KEYPAD_EQUAL => "NumpadEquals".into(),
        // ANSI letter, digit, and punctuation keys.
        KeyCode::ANSI_A => "A".into(),
        KeyCode::ANSI_B => "B".into(),
        KeyCode::ANSI_C => "C".into(),
        KeyCode::ANSI_D => "D".into(),
        KeyCode::ANSI_E => "E".into(),
        KeyCode::ANSI_F => "F".into(),
        KeyCode::ANSI_G => "G".into(),
        KeyCode::ANSI_H => "H".into(),
        KeyCode::ANSI_I => "I".into(),
        KeyCode::ANSI_J => "J".into(),
        KeyCode::ANSI_K => "K".into(),
        KeyCode::ANSI_L => "L".into(),
        KeyCode::ANSI_M => "M".into(),
        KeyCode::ANSI_N => "N".into(),
        KeyCode::ANSI_O => "O".into(),
        KeyCode::ANSI_P => "P".into(),
        KeyCode::ANSI_Q => "Q".into(),
        KeyCode::ANSI_R => "R".into(),
        KeyCode::ANSI_S => "S".into(),
        KeyCode::ANSI_T => "T".into(),
        KeyCode::ANSI_U => "U".into(),
        KeyCode::ANSI_V => "V".into(),
        KeyCode::ANSI_W => "W".into(),
        KeyCode::ANSI_X => "X".into(),
        KeyCode::ANSI_Y => "Y".into(),
        KeyCode::ANSI_Z => "Z".into(),
        KeyCode::ANSI_0 => "0".into(),
        KeyCode::ANSI_1 => "1".into(),
        KeyCode::ANSI_2 => "2".into(),
        KeyCode::ANSI_3 => "3".into(),
        KeyCode::ANSI_4 => "4".into(),
        KeyCode::ANSI_5 => "5".into(),
        KeyCode::ANSI_6 => "6".into(),
        KeyCode::ANSI_7 => "7".into(),
        KeyCode::ANSI_8 => "8".into(),
        KeyCode::ANSI_9 => "9".into(),
        KeyCode::ANSI_MINUS => "Minus".into(),
        KeyCode::ANSI_EQUAL => "Equal".into(),
        KeyCode::ANSI_LEFT_BRACKET => "BracketLeft".into(),
        KeyCode::ANSI_RIGHT_BRACKET => "BracketRight".into(),
        KeyCode::ANSI_BACKSLASH => "Backslash".into(),
        KeyCode::ANSI_SEMICOLON => "Semicolon".into(),
        KeyCode::ANSI_QUOTE => "Quote".into(),
        KeyCode::ANSI_GRAVE => "Grave".into(),
        KeyCode::ANSI_COMMA => "Comma".into(),
        KeyCode::ANSI_PERIOD => "Period".into(),
        KeyCode::ANSI_SLASH => "Slash".into(),
        // Modifiers and navigation used by the emergency bypass and layouts.
        KeyCode::ESCAPE => "Escape".into(),
        KeyCode::SHIFT | KeyCode::RIGHT_SHIFT => "Shift".into(),
        KeyCode::CONTROL | KeyCode::RIGHT_CONTROL => "Control".into(),
        KeyCode::OPTION | KeyCode::RIGHT_OPTION => "Option".into(),
        KeyCode::COMMAND | KeyCode::RIGHT_COMMAND => "Command".into(),
        KeyCode::RETURN => "Return".into(),
        KeyCode::TAB => "Tab".into(),
        KeyCode::SPACE => "Space".into(),
        KeyCode::DELETE => "Delete".into(),
        KeyCode::FORWARD_DELETE => "ForwardDelete".into(),
        KeyCode::HOME => "Home".into(),
        KeyCode::END => "End".into(),
        KeyCode::PAGE_UP => "PageUp".into(),
        KeyCode::PAGE_DOWN => "PageDown".into(),
        KeyCode::UP_ARROW => "ArrowUp".into(),
        KeyCode::DOWN_ARROW => "ArrowDown".into(),
        KeyCode::LEFT_ARROW => "ArrowLeft".into(),
        KeyCode::RIGHT_ARROW => "ArrowRight".into(),
        _ => format!("Unknown(0x{code:02x})"),
    }
}

/// Resolves a canonical physical-code name back to a virtual keycode.
///
/// Returns `None` for unknown or ambiguous names (for example a bare
/// `"Shift"`, which maps to two keycodes, is disambiguated to the left one).
#[must_use]
pub fn from_physical_name(name: &str) -> Option<CGKeyCode> {
    Some(match name {
        "Numpad0" => KeyCode::ANSI_KEYPAD_0,
        "Numpad1" => KeyCode::ANSI_KEYPAD_1,
        "Numpad2" => KeyCode::ANSI_KEYPAD_2,
        "Numpad3" => KeyCode::ANSI_KEYPAD_3,
        "Numpad4" => KeyCode::ANSI_KEYPAD_4,
        "Numpad5" => KeyCode::ANSI_KEYPAD_5,
        "Numpad6" => KeyCode::ANSI_KEYPAD_6,
        "Numpad7" => KeyCode::ANSI_KEYPAD_7,
        "Numpad8" => KeyCode::ANSI_KEYPAD_8,
        "Numpad9" => KeyCode::ANSI_KEYPAD_9,
        "NumpadAdd" => KeyCode::ANSI_KEYPAD_PLUS,
        "NumpadSubtract" => KeyCode::ANSI_KEYPAD_MINUS,
        "NumpadMultiply" => KeyCode::ANSI_KEYPAD_MULTIPLY,
        "NumpadDivide" => KeyCode::ANSI_KEYPAD_DIVIDE,
        "NumpadDecimal" => KeyCode::ANSI_KEYPAD_DECIMAL,
        "NumpadEnter" => KeyCode::ANSI_KEYPAD_ENTER,
        "NumLock" => KeyCode::ANSI_KEYPAD_CLEAR,
        "NumpadEquals" => KeyCode::ANSI_KEYPAD_EQUAL,
        "A" => KeyCode::ANSI_A,
        "B" => KeyCode::ANSI_B,
        "C" => KeyCode::ANSI_C,
        "D" => KeyCode::ANSI_D,
        "E" => KeyCode::ANSI_E,
        "F" => KeyCode::ANSI_F,
        "G" => KeyCode::ANSI_G,
        "H" => KeyCode::ANSI_H,
        "I" => KeyCode::ANSI_I,
        "J" => KeyCode::ANSI_J,
        "K" => KeyCode::ANSI_K,
        "L" => KeyCode::ANSI_L,
        "M" => KeyCode::ANSI_M,
        "N" => KeyCode::ANSI_N,
        "O" => KeyCode::ANSI_O,
        "P" => KeyCode::ANSI_P,
        "Q" => KeyCode::ANSI_Q,
        "R" => KeyCode::ANSI_R,
        "S" => KeyCode::ANSI_S,
        "T" => KeyCode::ANSI_T,
        "U" => KeyCode::ANSI_U,
        "V" => KeyCode::ANSI_V,
        "W" => KeyCode::ANSI_W,
        "X" => KeyCode::ANSI_X,
        "Y" => KeyCode::ANSI_Y,
        "Z" => KeyCode::ANSI_Z,
        "0" => KeyCode::ANSI_0,
        "1" => KeyCode::ANSI_1,
        "2" => KeyCode::ANSI_2,
        "3" => KeyCode::ANSI_3,
        "4" => KeyCode::ANSI_4,
        "5" => KeyCode::ANSI_5,
        "6" => KeyCode::ANSI_6,
        "7" => KeyCode::ANSI_7,
        "8" => KeyCode::ANSI_8,
        "9" => KeyCode::ANSI_9,
        "Escape" => KeyCode::ESCAPE,
        "Shift" => KeyCode::SHIFT,
        "Control" => KeyCode::CONTROL,
        "Option" => KeyCode::OPTION,
        "Command" => KeyCode::COMMAND,
        "Return" => KeyCode::RETURN,
        "Tab" => KeyCode::TAB,
        "Space" => KeyCode::SPACE,
        "Delete" => KeyCode::DELETE,
        "ForwardDelete" => KeyCode::FORWARD_DELETE,
        "Home" => KeyCode::HOME,
        "End" => KeyCode::END,
        "PageUp" => KeyCode::PAGE_UP,
        "PageDown" => KeyCode::PAGE_DOWN,
        "ArrowUp" => KeyCode::UP_ARROW,
        "ArrowDown" => KeyCode::DOWN_ARROW,
        "ArrowLeft" => KeyCode::LEFT_ARROW,
        "ArrowRight" => KeyCode::RIGHT_ARROW,
        _ => return None,
    })
}

/// Returns whether `name` is one of the canonical numpad codes.
#[must_use]
pub fn is_numpad(name: &str) -> bool {
    matches!(
        name,
        "Numpad0"
            | "Numpad1"
            | "Numpad2"
            | "Numpad3"
            | "Numpad4"
            | "Numpad5"
            | "Numpad6"
            | "Numpad7"
            | "Numpad8"
            | "Numpad9"
            | "NumpadAdd"
            | "NumpadSubtract"
            | "NumpadMultiply"
            | "NumpadDivide"
            | "NumpadDecimal"
            | "NumpadEnter"
            | "NumpadEquals"
            | "NumLock"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numpad_keycodes_map_to_canonical_names() {
        assert_eq!(physical_name(KeyCode::ANSI_KEYPAD_5), "Numpad5");
        assert_eq!(physical_name(KeyCode::ANSI_KEYPAD_0), "Numpad0");
        assert_eq!(physical_name(KeyCode::ANSI_KEYPAD_PLUS), "NumpadAdd");
        assert_eq!(physical_name(KeyCode::ANSI_KEYPAD_ENTER), "NumpadEnter");
        assert_eq!(physical_name(KeyCode::ANSI_KEYPAD_CLEAR), "NumLock");
        assert_eq!(physical_name(KeyCode::ESCAPE), "Escape");
    }

    #[test]
    fn unknown_keycodes_fall_back_to_a_stable_label() {
        assert_eq!(physical_name(0xF4), "Unknown(0xf4)");
    }

    #[test]
    fn canonical_names_resolve_back_to_keycodes() {
        assert_eq!(from_physical_name("Numpad5"), Some(KeyCode::ANSI_KEYPAD_5));
        assert_eq!(from_physical_name("Numpad0"), Some(KeyCode::ANSI_KEYPAD_0));
        assert_eq!(from_physical_name("Escape"), Some(KeyCode::ESCAPE));
        assert_eq!(from_physical_name("not-a-key"), None);
    }

    #[test]
    fn numpad_detection_covers_the_spec_layout() {
        assert!(is_numpad("Numpad5"));
        assert!(is_numpad("NumLock"));
        assert!(!is_numpad("Escape"));
        assert!(!is_numpad("A"));
    }

    #[test]
    fn mapping_round_trips_for_all_numpad_codes() {
        for name in [
            "Numpad0",
            "Numpad1",
            "Numpad2",
            "Numpad3",
            "Numpad4",
            "Numpad5",
            "Numpad6",
            "Numpad7",
            "Numpad8",
            "Numpad9",
            "NumpadAdd",
            "NumpadSubtract",
            "NumpadMultiply",
            "NumpadDivide",
            "NumpadDecimal",
            "NumpadEnter",
            "NumLock",
        ] {
            let code = from_physical_name(name).expect("numpad name resolves");
            assert_eq!(physical_name(code), name);
        }
    }
}
