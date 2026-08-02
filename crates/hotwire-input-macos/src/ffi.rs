//! Minimal, audited Quartz/ApplicationServices FFI.
//!
//! The [`core_graphics`] crate wraps most of the event-tap surface in safe
//! Rust. This module is the small, deliberate exception: the handful of entry
//! points Hotwire needs that the wrapper does not expose. The `unsafe_code`
//! lint is relaxed *only* here; every other module in this crate is safe Rust.
//!
//! # Safety
//!
//! - `AXIsProcessTrusted` takes no arguments and returns a value; it is safe
//!   to call from any thread and does not retain anything.
//! - `CGEventGetTimestamp` reads a 64-bit field off an event that the caller
//!   keeps alive for the duration of the call; the returned value is owned.
//! - `kCFRunLoopDefaultMode` / `kCFRunLoopCommonModes` are immutable
//!   `CFStringRef` constants owned by the framework; they are only read as
//!   `Copy` pointer values, never dereferenced or released here.

#![allow(unsafe_code)]

use std::ffi::c_void;

use core_foundation::runloop::{kCFRunLoopCommonModes, kCFRunLoopDefaultMode, CFRunLoopMode};
use core_graphics::event::CGEvent;
use foreign_types::ForeignType;

/// Returns whether the current process has Accessibility trust.
///
/// Event taps and event posting both require this permission. This is a
/// pre-flight check; the authoritative signal is still whether
/// `CGEventTapCreate` succeeds.
#[must_use]
pub fn process_is_trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// Returns the event timestamp in nanoseconds since system startup.
///
/// `CGEventTimestamp` is already elapsed nanoseconds (not microseconds), so it
/// is assigned to `PhysicalKeyEvent::timestamp_ns` directly without scaling.
#[must_use]
pub fn event_timestamp_ns(event: &CGEvent) -> u64 {
    unsafe { CGEventGetTimestamp(event.as_ptr() as *const c_void) }
}

/// Overrides the timestamp of an event. Used by tests to pin an exact
/// nanosecond value and establish the unit contract.
#[cfg(test)]
pub fn set_event_timestamp(event: &CGEvent, timestamp_ns: u64) {
    unsafe { CGEventSetTimestamp(event.as_ptr() as *const c_void, timestamp_ns) }
}

/// Returns the run-loop mode covering normal and modal sessions, the mode the
/// event-tap source must be added in.
#[must_use]
pub fn common_modes() -> CFRunLoopMode {
    unsafe { kCFRunLoopCommonModes }
}

/// Returns the default run-loop mode.
#[must_use]
pub fn default_mode() -> CFRunLoopMode {
    unsafe { kCFRunLoopDefaultMode }
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventGetTimestamp(event: *const c_void) -> u64;
    #[cfg(test)]
    fn CGEventSetTimestamp(event: *const c_void, timestamp: u64);
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
}
