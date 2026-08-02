//! Guarded interactive harness for the macOS Quartz event-tap proof.
//!
//! Run it, then follow the on-screen instructions to demonstrate that a bound
//! numpad key is consumed while unmatched keys pass through, and that the
//! `Control` + `Option` + `Command` + `Escape` chord pauses and resumes capture.
//!
//! ```text
//! cargo run -p hotwire-input-macos --example probe
//! ```
//!
//! The harness refuses to run unless `HOTWIRE_PROBE=1` is set, because starting
//! it really does consume keyboard events:
//!
//! ```text
//! HOTWIRE_PROBE=1 cargo run -p hotwire-input-macos --example probe
//! ```
//!
//! It also requires the Accessibility permission for the terminal that runs it
//! (System Settings > Privacy & Security > Accessibility).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hotwire_input::CaptureMode;
use hotwire_input_macos::{PermissionStatus, QuartzEventTap, TapDecision, TapStatus};

/// Refuses to run unless this is `1`.
const GUARD_ENV: &str = "HOTWIRE_PROBE";

/// The keys the probe binds by default.
const DEFAULT_KEYS: [&str; 2] = ["Numpad5", "Numpad0"];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var(GUARD_ENV).ok().as_deref() != Some("1") {
        eprintln!("refusing to start: set {GUARD_ENV}=1 to confirm this harness may consume keys");
        std::process::exit(2);
    }

    let mut keys = std::env::args().skip(1).collect::<Vec<_>>();
    keys.extend(DEFAULT_KEYS.into_iter().map(str::to_owned));
    let keys = dedupe(keys);

    let tap = QuartzEventTap::new();
    if tap.permission_status() == PermissionStatus::Denied {
        eprintln!("Accessibility permission is not granted for this process.");
        eprintln!("  Open System Settings > Privacy & Security > Accessibility and enable it for");
        eprintln!("  your terminal, then re-run. Hotwire listens only to bound keys, processes");
        eprintln!("  events locally, stores no typed text, and you can revoke it at any time.");
        std::process::exit(1);
    }

    let (decision_tx, decision_rx) = std::sync::mpsc::channel();
    tap.set_decision_sink(decision_tx);
    tap.set_capture_mode(CaptureMode::Capture);
    tap.set_captured_keys(&keys);

    let (event_tx, _event_rx) = std::sync::mpsc::channel();
    tap.start(event_tx)?;
    wait_until_running(&tap)?;

    let running = Arc::new(AtomicBool::new(true));
    ctrlc::set_handler({
        let running = Arc::clone(&running);
        move || running.store(false, Ordering::SeqCst)
    })?;

    print_intro(&keys);
    run_loop(&tap, &decision_rx, &running);

    tap.stop();
    println!("\nTap stopped. All Hotwire-held keys released; capture is fail-open.");
    Ok(())
}

/// Prints the menu a human follows while pressing keys.
fn print_intro(keys: &[String]) {
    println!("\nHotwire Quartz event-tap probe");
    println!("  Tap status : Running");
    println!("  Captured   : {}", keys.join(", "));
    println!("\nInstructions (in any order):");
    println!(
        "  - Press a captured key ({}): expect [SUPPRESS] — the key will NOT reach other apps.",
        keys.join(" or ")
    );
    println!("  - Press any other key (e.g. A): expect [PASS] — it reaches other apps normally.");
    println!(
        "  - Hold Control + Option + Command and press Escape: expect an [EMERGENCY] pause line;"
    );
    println!("    while paused every key passes through. Press the chord again to resume.");
    println!("  - Ctrl+C to quit; shutdown releases any held keys and capture stops.");
    println!();
}

/// Drains decisions until interrupted or the channel disconnects.
fn run_loop(tap: &QuartzEventTap, rx: &Receiver<TapDecision>, running: &AtomicBool) {
    let mut last_paused = tap.is_paused();
    let mut last_status = tap.status();

    while running.load(Ordering::SeqCst) {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(decision) => {
                if decision.paused != last_paused {
                    let verb = if decision.paused { "paused" } else { "resumed" };
                    let note = if decision.paused {
                        "all keys pass through"
                    } else {
                        "keys are consumed again"
                    };
                    println!("[EMERGENCY] capture {verb} — {note}");
                    last_paused = decision.paused;
                }
                print_decision(&decision);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }

        let status = tap.status();
        if status != last_status {
            println!("[TAP] status: {status:?}");
            last_status = status;
        }
    }
}

/// Renders one decision line.
fn print_decision(decision: &TapDecision) {
    let action = if decision.suppressed {
        "SUPPRESS"
    } else {
        "PASS"
    };
    let state = match decision.event.state {
        hotwire_core::KeyState::Down => "down",
        hotwire_core::KeyState::Up => "up",
    };
    let repeat = if decision.event.is_repeat {
        " (repeat)"
    } else {
        ""
    };
    println!(
        "[{action}] {} {state}{repeat}  scan=0x{:x}",
        decision.event.physical_code, decision.event.scan_code
    );
}

/// Waits for the tap thread to report running, surfacing startup failures.
fn wait_until_running(tap: &QuartzEventTap) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        match tap.status() {
            TapStatus::Running => return Ok(()),
            TapStatus::StartFailed => {
                return Err("the Quartz event tap could not be created".to_string());
            }
            _ => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    Err("timed out waiting for the event tap to start".to_string())
}

/// Removes duplicate keys while preserving the given order.
fn dedupe(keys: Vec<String>) -> Vec<String> {
    let mut seen = Vec::with_capacity(keys.len());
    for key in keys {
        if !seen.contains(&key) {
            seen.push(key);
        }
    }
    seen
}
