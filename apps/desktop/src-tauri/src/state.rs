//! Shell state and recovery controls.
//!
//! The desktop shell owns one shared [`QuartzEventTap`] (capture is started by
//! the app when permissions allow; until then the gate is inert and fails
//! open), the menu-bar pause item, the last action receipt, and the adapter
//! execution surface ([`crate::adapters::AdapterState`]).
//!
//! The pause/quit path here is the fail-open recovery surface (spec §15.5):
//! pausing or shutting down drives *both* surfaces — stop capture on the tap
//! and cancel/release every active adapter hold — so no key stays logically
//! held and no action keeps running. Recovery ordering is fail-open: capture is
//! stopped first so no new input can start actions while we unwind, then the
//! active adapter holds are released. Every lifecycle operation is idempotent:
//! a second pause or shutdown releases nothing. When shell executions land
//! (the `hotwire-runner` `CommandRunner`), the same lifecycle is the single
//! place their cancellations join the adapter holds and the tap.

use std::sync::Mutex;

use hotwire_core::ActionReceipt;
use hotwire_input_macos::QuartzEventTap;
use tauri::menu::MenuItem;

use crate::adapters::AdapterState;

/// Menu id for the "Pause capture" / "Resume capture" item.
pub const MENU_PAUSE: &str = "pause";
/// Label shown while capture is running.
pub const PAUSE_LABEL: &str = "Pause capture";
/// Label shown while capture is paused.
pub const RESUME_LABEL: &str = "Resume capture";

/// The lifecycle flags that make recovery idempotent.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Lifecycle {
    /// A pause was requested and its adapter holds have been released.
    paused: bool,
    /// The shell has been shut down and must never run again.
    stopped: bool,
}

/// Process-wide state managed by the Tauri shell.
pub struct ShellState {
    /// The shared macOS capture tap (created but not started by default).
    pub tap: QuartzEventTap,
    /// The menu-bar pause item, so its label tracks the pause state.
    pub pause_item: MenuItem<tauri::Wry>,
    /// The most recent action receipt, for the diagnostics "last action".
    pub last_receipt: Mutex<Option<ActionReceipt>>,
    /// The adapter execution surface (ADP-001 vertical slice).
    pub adapters: AdapterState,
    /// Idempotence flags for the pause/shutdown lifecycle.
    pub lifecycle: Mutex<Lifecycle>,
}

impl ShellState {
    /// Builds shell state around an existing tap, pause item, and adapter
    /// surface.
    #[must_use]
    pub fn new(
        tap: QuartzEventTap,
        pause_item: MenuItem<tauri::Wry>,
        adapters: AdapterState,
    ) -> Self {
        Self {
            tap,
            pause_item,
            last_receipt: Mutex::new(None),
            adapters,
            lifecycle: Mutex::new(Lifecycle::default()),
        }
    }

    /// Whether the shell is currently paused (its adapter holds released).
    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.lifecycle.lock().expect("lifecycle lock").paused
    }

    /// Pauses the shell: stops capture on the tap and cancels/releases every
    /// active adapter hold. Idempotent — a second pause releases nothing.
    ///
    /// Returns how many adapter holds were released.
    pub async fn pause(&self) -> usize {
        let released = pause_recovery(&self.tap, &self.adapters, &self.lifecycle).await;
        sync_pause_label(&self.pause_item, self.tap.is_paused());
        released
    }

    /// Resumes the shell after a pause (adapter holds stay released).
    ///
    /// Returns the new paused state (`false` when resumed).
    pub fn resume(&self) -> bool {
        let paused = resume_recovery(&self.tap, &self.lifecycle);
        sync_pause_label(&self.pause_item, paused);
        paused
    }

    /// Shuts the shell down: stops capture (releasing any injected keys) and
    /// cancels/releases every active adapter hold. Idempotent.
    ///
    /// Returns how many adapter holds were released.
    pub async fn shutdown(&self) -> usize {
        shutdown_recovery(&self.tap, &self.adapters, &self.lifecycle).await
    }
}

/// Pauses recovery across both surfaces, testable without a window or menu.
///
/// Fail-open ordering: capture is paused before any adapter hold is released.
/// Idempotent via `lifecycle.paused`.
pub async fn pause_recovery(
    tap: &QuartzEventTap,
    adapters: &AdapterState,
    lifecycle: &Mutex<Lifecycle>,
) -> usize {
    {
        let mut lifecycle = lifecycle.lock().expect("lifecycle lock");
        if lifecycle.paused {
            // Already paused: still fail-open, but release nothing again.
            drop(lifecycle);
            pause_capture(tap);
            return 0;
        }
        lifecycle.paused = true;
    }
    pause_capture(tap);
    adapters.release_active().await
}

/// Resumes recovery on the tap, clearing the paused flag.
pub fn resume_recovery(tap: &QuartzEventTap, lifecycle: &Mutex<Lifecycle>) -> bool {
    let mut lifecycle = lifecycle.lock().expect("lifecycle lock");
    if lifecycle.stopped {
        return tap.is_paused();
    }
    lifecycle.paused = false;
    resume_capture(tap)
}

/// Shutdown recovery across both surfaces, testable without a window or menu.
///
/// Fail-open ordering: the tap is stopped (which releases any keys it
/// injected) before active adapter holds are cancelled. Idempotent via
/// `lifecycle.stopped`.
pub async fn shutdown_recovery(
    tap: &QuartzEventTap,
    adapters: &AdapterState,
    lifecycle: &Mutex<Lifecycle>,
) -> usize {
    {
        let mut lifecycle = lifecycle.lock().expect("lifecycle lock");
        if lifecycle.stopped {
            return 0;
        }
        lifecycle.stopped = true;
        lifecycle.paused = true;
    }
    tap.stop();
    adapters.release_active().await
}

/// Creates the menu-bar pause item.
///
/// # Errors
///
/// Returns the underlying Tauri error when the item cannot be created.
pub fn create_pause_item(app: &tauri::AppHandle) -> tauri::Result<MenuItem<tauri::Wry>> {
    MenuItem::with_id(app, MENU_PAUSE, PAUSE_LABEL, true, None::<&str>)
}

/// Pauses capture (fail-open). Returns the new paused state.
pub fn pause_capture(tap: &QuartzEventTap) -> bool {
    tap.emergency_pause();
    tap.is_paused()
}

/// Resumes capture after a pause. Returns the new paused state.
pub fn resume_capture(tap: &QuartzEventTap) -> bool {
    tap.emergency_resume();
    tap.is_paused()
}

/// Re-labels the menu-bar pause item to match the paused state.
pub fn sync_pause_label(pause_item: &MenuItem<tauri::Wry>, paused: bool) {
    let _ = pause_item.set_text(if paused { RESUME_LABEL } else { PAUSE_LABEL });
}

/// Maps the last action receipt into the diagnostics summary.
#[must_use]
pub fn summarize_last_receipt(
    receipt: &Option<ActionReceipt>,
) -> Option<hotwire_core::ActionSummary> {
    receipt.as_ref().map(|receipt| hotwire_core::ActionSummary {
        action_id: receipt.action_id.clone(),
        adapter_id: receipt.adapter_id.clone(),
        status: receipt.status.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use async_trait::async_trait;
    use hotwire_adapter_papegoye::{PapegoyeAdapter, PapegoyeError, PapegoyePlatform};
    use hotwire_core::Trigger;
    use hotwire_router::AdapterRegistry;
    use serde_json::json;

    /// Records every injected key event so recovery ordering is observable.
    #[derive(Default)]
    struct Recorded {
        downs: Mutex<Vec<u16>>,
        ups: Mutex<Vec<u16>>,
        held: Mutex<Vec<u16>>,
    }

    /// A Papegøye platform that only records — no real keyboard is touched.
    struct MockPapegoye {
        present: AtomicBool,
        recorded: Arc<Recorded>,
    }

    impl MockPapegoye {
        fn new() -> (Self, Arc<Recorded>) {
            let recorded = Arc::new(Recorded::default());
            (
                Self {
                    present: AtomicBool::new(true),
                    recorded: recorded.clone(),
                },
                recorded,
            )
        }
    }

    #[async_trait]
    impl PapegoyePlatform for MockPapegoye {
        fn resolve_key(&self, name: &str) -> Option<u16> {
            match name {
                "space" => Some(0x31),
                "F17" => Some(0x40),
                _ => None,
            }
        }

        fn resolve_modifier(&self, name: &str) -> Option<u16> {
            match name {
                "fn" => Some(0x3F),
                _ => None,
            }
        }

        fn app_available(&self) -> bool {
            self.present.load(Ordering::Relaxed)
        }

        async fn key_down(&self, keycode: u16) -> Result<(), PapegoyeError> {
            self.recorded.downs.lock().expect("lock").push(keycode);
            let mut held = self.recorded.held.lock().expect("lock");
            if !held.contains(&keycode) {
                held.push(keycode);
            }
            Ok(())
        }

        async fn key_up(&self, keycode: u16) -> Result<(), PapegoyeError> {
            self.recorded.ups.lock().expect("lock").push(keycode);
            self.recorded
                .held
                .lock()
                .expect("lock")
                .retain(|code| *code != keycode);
            Ok(())
        }

        async fn release_all(&self) -> Vec<u16> {
            self.recorded.held.lock().expect("lock").drain(..).collect()
        }
    }

    /// A tap + adapter surface wired to a recording Papegøye adapter.
    fn recovery_fixture() -> (QuartzEventTap, AdapterState, Arc<Recorded>) {
        let tap = QuartzEventTap::new();
        let (platform, recorded) = MockPapegoye::new();
        let mut registry = AdapterRegistry::new();
        registry
            .register(Arc::new(PapegoyeAdapter::new(Arc::new(platform))))
            .expect("registers");
        (tap, AdapterState::with_registry(registry), recorded)
    }

    /// Holds the push-to-talk key through the real adapter (tracked as active).
    async fn start_hold(adapters: &AdapterState) -> ActionReceipt {
        adapters
            .run(
                "papegoye",
                "voice.input",
                Trigger::Hold,
                json!({ "shortcut": "fn+space" }),
                "Numpad0",
            )
            .await
    }

    #[tokio::test]
    async fn pause_releases_active_holds_and_is_idempotent() {
        let (tap, adapters, recorded) = recovery_fixture();
        let lifecycle = Mutex::new(Lifecycle::default());

        let started = start_hold(&adapters).await;
        assert_eq!(started.status, hotwire_core::ActionStatus::Started);
        assert_eq!(adapters.active_count(), 1);

        // Pause stops capture and releases the hold, key before modifiers.
        assert!(!tap.is_paused());
        assert_eq!(pause_recovery(&tap, &adapters, &lifecycle).await, 1);
        assert!(tap.is_paused(), "capture stops on pause (fail-open)");
        assert_eq!(adapters.active_count(), 0, "no adapter hold survives pause");
        assert_eq!(
            recorded.downs.lock().expect("lock").as_slice(),
            &[0x3F, 0x31]
        );
        assert_eq!(
            recorded.ups.lock().expect("lock").as_slice(),
            &[0x31, 0x3F],
            "release order stays balanced: key before modifiers"
        );

        // A second pause releases nothing.
        assert_eq!(pause_recovery(&tap, &adapters, &lifecycle).await, 0);
        assert_eq!(
            recorded.ups.lock().expect("lock").len(),
            2,
            "no duplicate ups"
        );
    }

    #[tokio::test]
    async fn shutdown_releases_active_holds_and_is_idempotent() {
        let (tap, adapters, recorded) = recovery_fixture();
        let lifecycle = Mutex::new(Lifecycle::default());

        let started = start_hold(&adapters).await;
        assert_eq!(started.status, hotwire_core::ActionStatus::Started);
        assert_eq!(adapters.active_count(), 1);

        assert_eq!(shutdown_recovery(&tap, &adapters, &lifecycle).await, 1);
        assert_eq!(tap.status(), hotwire_input_macos::TapStatus::Stopped);
        assert_eq!(
            adapters.active_count(),
            0,
            "no adapter hold survives shutdown"
        );
        assert_eq!(recorded.downs.lock().expect("lock").len(), 2);
        assert_eq!(recorded.ups.lock().expect("lock").len(), 2, "balanced ups");
        assert_eq!(recorded.ups.lock().expect("lock").as_slice(), &[0x31, 0x3F]);

        // A second shutdown releases nothing.
        assert_eq!(shutdown_recovery(&tap, &adapters, &lifecycle).await, 0);
        assert_eq!(
            recorded.ups.lock().expect("lock").len(),
            2,
            "no duplicate ups"
        );
    }

    #[tokio::test]
    async fn shutdown_after_pause_is_a_noop() {
        let (tap, adapters, recorded) = recovery_fixture();
        let lifecycle = Mutex::new(Lifecycle::default());

        start_hold(&adapters).await;
        assert_eq!(pause_recovery(&tap, &adapters, &lifecycle).await, 1);

        // A later shutdown must not double-release the already-freed holds.
        assert_eq!(shutdown_recovery(&tap, &adapters, &lifecycle).await, 0);
        assert_eq!(
            recorded.ups.lock().expect("lock").len(),
            2,
            "exactly one release"
        );
    }

    #[test]
    fn pause_and_resume_toggle_the_shared_tap() {
        let tap = QuartzEventTap::new();
        assert!(!tap.is_paused());

        assert!(pause_capture(&tap));
        assert!(tap.is_paused());
        assert!(tap.health().fail_open(), "pausing must fail open");

        assert!(!resume_capture(&tap));
        assert!(!tap.is_paused());
    }

    #[test]
    fn pause_labels_are_stable() {
        assert_eq!(MENU_PAUSE, "pause");
        assert_eq!(PAUSE_LABEL, "Pause capture");
        assert_eq!(RESUME_LABEL, "Resume capture");
    }

    #[test]
    fn last_receipt_maps_to_a_summary_without_sensitive_detail() {
        let receipt = crate::commands::mock_receipt();
        let summary = summarize_last_receipt(&Some(receipt)).expect("summary");
        assert_eq!(summary.action_id, "app.open_or_focus");
        assert_eq!(summary.adapter_id, "herdr");
        assert_eq!(summary.status, hotwire_core::ActionStatus::Succeeded);
        assert_eq!(summarize_last_receipt(&None), None);
    }
}
