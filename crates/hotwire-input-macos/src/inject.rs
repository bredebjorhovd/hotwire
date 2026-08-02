//! Marker-tagged keyboard-event injection for Hotwire-generated input.
//!
//! The shortcut adapter will use this to synthesize keystrokes. Every injected
//! event carries [`INJECTED_MARKER`] in its user-data field so the capture tap
//! recognizes it as Hotwire's own and passes it through untouched — this is
//! the injection-loop prevention invariant.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard};

use core_graphics::event::{CGEvent, CGEventTapLocation, EventField};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use thiserror::Error;

use crate::keycode;

/// Recovers a poisoned lock: the held-key critical sections never panic, so a
/// poisoned mutex can only mean a prior test panic, and the values remain
/// usable.
fn held_lock(mutex: &Mutex<HashSet<u16>>) -> MutexGuard<'_, HashSet<u16>> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// `"HOTWIRE"` as an ASCII tag written into the user-data field of injected
/// events so the capture tap can filter them.
pub const INJECTED_MARKER: u64 = 0x0048_4F54_5749_5245;

/// Errors produced while injecting keyboard events.
#[derive(Debug, Error)]
pub enum InjectError {
    /// The event source or event could not be created.
    #[error("failed to create a Quartz keyboard event")]
    Create,
    /// A physical-code name did not resolve to a keycode.
    #[error("unknown physical code `{0}`")]
    UnknownKey(String),
}

/// Synthesizes keyboard events, tags them as Hotwire's, and tracks which keys
/// are currently held so shutdown can never leave a logical key down.
#[derive(Clone, Debug)]
pub struct MacEventInjector {
    marker: u64,
    held: Arc<Mutex<HashSet<u16>>>,
}

impl MacEventInjector {
    /// Creates an injector that tags events with `marker`.
    #[must_use]
    pub fn new(marker: u64) -> Self {
        Self {
            marker,
            held: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Presses `keycode` down, tagging the event as Hotwire's own.
    ///
    /// # Errors
    ///
    /// Returns [`InjectError::Create`] when the OS refuses to build the event.
    pub fn key_down(&self, keycode: u16) -> Result<(), InjectError> {
        self.post_key(keycode, true)?;
        self.record_down(keycode);
        Ok(())
    }

    /// Releases `keycode`, tagging the event as Hotwire's own.
    ///
    /// # Errors
    ///
    /// Returns [`InjectError::Create`] when the OS refuses to build the event.
    pub fn key_up(&self, keycode: u16) -> Result<(), InjectError> {
        self.post_key(keycode, false)?;
        self.record_up(keycode);
        Ok(())
    }

    /// Presses the key named by a canonical physical code.
    ///
    /// # Errors
    ///
    /// Returns [`InjectError::UnknownKey`] when `name` does not resolve and
    /// [`InjectError::Create`] when the OS refuses to build the event.
    pub fn key_down_named(&self, name: &str) -> Result<(), InjectError> {
        let code = keycode::from_physical_name(name)
            .ok_or_else(|| InjectError::UnknownKey(name.to_string()))?;
        self.key_down(code)
    }

    /// Releases the key named by a canonical physical code.
    ///
    /// # Errors
    ///
    /// Returns [`InjectError::UnknownKey`] when `name` does not resolve and
    /// [`InjectError::Create`] when the OS refuses to build the event.
    pub fn key_up_named(&self, name: &str) -> Result<(), InjectError> {
        let code = keycode::from_physical_name(name)
            .ok_or_else(|| InjectError::UnknownKey(name.to_string()))?;
        self.key_up(code)
    }

    /// Returns the keycodes Hotwire currently holds down.
    #[must_use]
    pub fn held_keys(&self) -> Vec<u16> {
        held_lock(&self.held).iter().copied().collect()
    }

    /// Releases every key Hotwire currently holds, posting a key-up per key.
    ///
    /// Called on shutdown so a crashed interaction cannot leave a logical key
    /// down (fail-open invariant).
    #[must_use]
    pub fn release_all(&self) -> Vec<u16> {
        let held = held_lock(&self.held).drain().collect::<Vec<_>>();
        for keycode in &held {
            let _ = self.post_key(*keycode, false);
        }
        held
    }

    fn post_key(&self, keycode: u16, down: bool) -> Result<(), InjectError> {
        let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
            .map_err(|()| InjectError::Create)?;
        let event =
            CGEvent::new_keyboard_event(source, keycode, down).map_err(|()| InjectError::Create)?;
        event.set_integer_value_field(
            EventField::EVENT_SOURCE_USER_DATA,
            i64::try_from(self.marker).expect("injection marker fits in an i64"),
        );
        event.post(CGEventTapLocation::HID);
        Ok(())
    }

    fn record_down(&self, keycode: u16) {
        held_lock(&self.held).insert(keycode);
    }

    fn record_up(&self, keycode: u16) {
        held_lock(&self.held).remove(&keycode);
    }
}

impl Default for MacEventInjector {
    fn default() -> Self {
        Self::new(INJECTED_MARKER)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn held_key_tracking_records_and_releases() {
        let injector = MacEventInjector::new(INJECTED_MARKER);

        injector.record_down(0x52);
        injector.record_down(0x57);
        assert_eq!(injector.held_keys().len(), 2);

        injector.record_up(0x52);
        assert_eq!(injector.held_keys(), vec![0x57]);

        let released = injector.release_all();
        assert_eq!(released, vec![0x57]);
        assert!(injector.held_keys().is_empty());
    }

    #[test]
    fn releasing_an_unheld_key_is_a_noop() {
        let injector = MacEventInjector::new(INJECTED_MARKER);
        injector.record_up(0x52);
        assert!(injector.held_keys().is_empty());
    }
}
