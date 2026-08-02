//! Guarded macOS integration test for the Quartz event-tap proof.
//!
//! This test drives the real event pipeline: it starts a capture tap, posts
//! unmarked synthetic events through `CGEventPost`, and asserts what the tap
//! decided for each. It is deliberately opt-in:
//!
//! 1. It is `#[ignore]`d, so plain `cargo test` never runs it.
//! 2. It requires `HOTWIRE_INTEGRATION=1`.
//! 3. It requires Accessibility trust; otherwise it skips.
//!
//! Run it with:
//!
//! ```text
//! HOTWIRE_INTEGRATION=1 cargo test -p hotwire-input-macos --test event_tap -- --ignored
//! ```
//!
//! Posting a passthrough key (the `A` below) really delivers it to the active
//! application, so run this when you can spare a stray keystroke.

#![cfg(target_os = "macos")]

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, KeyCode};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use hotwire_core::KeyState;
use hotwire_input::CaptureMode;
use hotwire_input_macos::{PermissionStatus, QuartzEventTap, TapDecision, TapStatus};

const GUARD_ENV: &str = "HOTWIRE_INTEGRATION";

fn post_key(keycode: u16, down: bool, flags: CGEventFlags) {
    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
        .expect("event source creation should succeed");
    let event = CGEvent::new_keyboard_event(source, keycode, down)
        .expect("keyboard event creation should succeed");
    event.set_flags(flags);
    event.post(CGEventTapLocation::HID);
}

/// Posts a full press (down then up) of `keycode` and returns the decisions
/// recorded while the tap processed it.
fn press(keycode: u16, rx: &Receiver<TapDecision>) -> Vec<TapDecision> {
    post_key(keycode, true, CGEventFlags::default());
    let mut decisions = drain_for(rx, Duration::from_millis(400));
    post_key(keycode, false, CGEventFlags::default());
    decisions.extend(drain_for(rx, Duration::from_millis(400)));
    decisions
}

/// Drains whatever decisions arrive within `window`.
fn drain_for(rx: &Receiver<TapDecision>, window: Duration) -> Vec<TapDecision> {
    let deadline = Instant::now() + window;
    let mut decisions = Vec::new();
    while Instant::now() < deadline {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(decision) => decisions.push(decision),
            Err(_) => break,
        }
    }
    decisions
}

fn decisions_for<'a>(
    decisions: &'a [TapDecision],
    physical_code: &str,
    state: &KeyState,
) -> Vec<&'a TapDecision> {
    decisions
        .iter()
        .filter(|decision| {
            decision.event.physical_code == physical_code && decision.event.state == *state
        })
        .collect()
}

fn wait_until_running(tap: &QuartzEventTap) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        match tap.status() {
            TapStatus::Running => return,
            TapStatus::StartFailed => panic!("event tap failed to start"),
            _ => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    panic!("timed out waiting for the event tap to start");
}

#[test]
#[ignore = "requires Accessibility trust and HOTWIRE_INTEGRATION=1"]
fn event_tap_captures_bound_keys_and_passes_everything_else_through() {
    if std::env::var(GUARD_ENV).ok().as_deref() != Some("1") {
        eprintln!("skipping: set {GUARD_ENV}=1 to run the guarded integration test");
        return;
    }
    let tap = QuartzEventTap::new();
    if tap.permission_status() != PermissionStatus::Authorized {
        eprintln!("skipping: Accessibility permission is not granted for this process");
        return;
    }

    let (decision_tx, decision_rx) = std::sync::mpsc::channel();
    tap.set_decision_sink(decision_tx);
    tap.set_capture_mode(CaptureMode::Capture);
    tap.set_captured_keys(&["Numpad5".to_string()]);

    let (event_tx, _event_rx) = std::sync::mpsc::channel();
    tap.start(event_tx).expect("tap should start");
    wait_until_running(&tap);

    // Selected key (Numpad5) is consumed while capture is active.
    let decisions = press(KeyCode::ANSI_KEYPAD_5, &decision_rx);
    let down = decisions_for(&decisions, "Numpad5", &KeyState::Down);
    let up = decisions_for(&decisions, "Numpad5", &KeyState::Up);
    assert_eq!(down.len(), 1, "one Numpad5 down decision expected");
    assert_eq!(up.len(), 1, "one Numpad5 up decision expected");
    assert!(down[0].suppressed, "bound key down must be consumed");
    assert!(up[0].suppressed, "bound key up must be consumed");

    // Unmatched key passes through.
    let decisions = press(KeyCode::ANSI_A, &decision_rx);
    let down = decisions_for(&decisions, "A", &KeyState::Down);
    assert_eq!(down.len(), 1, "one A down decision expected");
    assert!(!down[0].suppressed, "unbound key must pass through");

    // Emergency bypass pauses capture; everything then passes through.
    post_key(KeyCode::ESCAPE, true, ctrl_opt_cmd());
    let decisions = drain_for(&decision_rx, Duration::from_millis(400));
    let escape = decisions_for(&decisions, "Escape", &KeyState::Down);
    assert_eq!(escape.len(), 1, "one Escape decision expected");
    assert!(escape[0].paused, "bypass chord must pause capture");

    let decisions = press(KeyCode::ANSI_KEYPAD_5, &decision_rx);
    let down = decisions_for(&decisions, "Numpad5", &KeyState::Down);
    assert_eq!(down.len(), 1);
    assert!(!down[0].suppressed, "capture must be paused (fail-open)");

    // The same chord resumes capture.
    post_key(KeyCode::ESCAPE, true, ctrl_opt_cmd());
    let decisions = drain_for(&decision_rx, Duration::from_millis(400));
    let escape = decisions_for(&decisions, "Escape", &KeyState::Down);
    assert_eq!(escape.len(), 1);
    assert!(!escape[0].paused, "second bypass press must resume capture");

    let decisions = press(KeyCode::ANSI_KEYPAD_5, &decision_rx);
    let down = decisions_for(&decisions, "Numpad5", &KeyState::Down);
    assert_eq!(down.len(), 1);
    assert!(down[0].suppressed, "capture must resume after the bypass");

    // Shutdown must not leave a logically held key down.
    let injector = tap.injector();
    injector
        .key_down(KeyCode::ANSI_KEYPAD_0)
        .expect("marker-tagged injection should post");
    assert!(injector.held_keys().contains(&KeyCode::ANSI_KEYPAD_0));

    tap.stop();
    assert!(
        injector.held_keys().is_empty(),
        "shutdown must release every logically held key"
    );
    assert_eq!(tap.status(), TapStatus::Stopped);
}

fn ctrl_opt_cmd() -> CGEventFlags {
    CGEventFlags::CGEventFlagControl
        | CGEventFlags::CGEventFlagAlternate
        | CGEventFlags::CGEventFlagCommand
}
