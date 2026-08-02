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
//! active adapter holds are released.
//!
//! Each complete lifecycle operation — pause, resume, shutdown — is serialized
//! behind one [`RecoveryGate`] owned by the shell, so the tap transition and
//! the adapter pause/shutdown state always move together. A concurrent resume
//! can never read `is_stopped()` as false and then restart capture after a
//! shutdown has already stopped the shell. Inside that gate,
//! [`AdapterState`] serializes the release against any in-progress start
//! (waits for it, then releases everything) and rejects fresh adapter starts
//! while paused or stopped. Every lifecycle operation is idempotent: a second
//! pause or shutdown releases nothing — but a hold whose release previously
//! failed stays tracked and is retried. When shell executions land (the
//! `hotwire-runner` `CommandRunner`), the same lifecycle is the single place
//! their cancellations join the adapter holds and the tap.

use std::sync::Mutex;

use hotwire_core::ActionReceipt;
use hotwire_input_macos::QuartzEventTap;
use hotwire_router::{BindingRouter, RouterConfig};
use tauri::menu::MenuItem;

use crate::adapters::AdapterState;

/// Menu id for the "Pause capture" / "Resume capture" item.
pub const MENU_PAUSE: &str = "pause";
/// Label shown while capture is running.
pub const PAUSE_LABEL: &str = "Pause capture";
/// Label shown while capture is paused.
pub const RESUME_LABEL: &str = "Resume capture";

/// Serializes each complete ShellState lifecycle operation (tap transition
/// plus adapter pause/resume/shutdown) so the two recovery surfaces can never
/// diverge under concurrency.
pub type RecoveryGate = tokio::sync::Mutex<()>;

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
    /// The validated active profile router fed by the native event tap.
    router: Mutex<BindingRouter>,
    /// Serializes the pause/resume/shutdown lifecycle (tap + adapter).
    recovery: RecoveryGate,
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
        let profile = hotwire_profile::parse_yaml(include_str!(
            "../../../../packages/profiles/fixtures/ai-numpad.yaml"
        ))
        .expect("canonical AI Numpad fixture must remain valid");
        Self {
            tap,
            pause_item,
            last_receipt: Mutex::new(None),
            adapters,
            router: Mutex::new(
                BindingRouter::new(profile, RouterConfig::default())
                    .expect("canonical AI Numpad fixture must be routable"),
            ),
            recovery: RecoveryGate::default(),
        }
    }

    /// Configures the native tap for the active profile's assigned controls.
    pub fn configure_capture(&self) {
        let profile = hotwire_profile::parse_yaml(include_str!(
            "../../../../packages/profiles/fixtures/ai-numpad.yaml"
        ))
        .expect("canonical AI Numpad fixture must remain valid");
        self.configure_profile(&profile);
    }

    /// Activates a validated profile and updates the native capture policy.
    pub fn activate_profile(&self, profile: hotwire_profile::Profile) -> Result<(), String> {
        let router = BindingRouter::new(profile.clone(), RouterConfig::default())
            .map_err(|error| error.to_string())?;
        *self.router.lock().expect("router lock") = router;
        self.configure_profile(&profile);
        Ok(())
    }

    fn configure_profile(&self, profile: &hotwire_profile::Profile) {
        let keys = profile
            .bindings
            .iter()
            .filter(|binding| binding.enabled)
            .map(|binding| binding.physical_code.clone())
            .collect::<Vec<_>>();
        self.tap.set_captured_keys(&keys);
        self.tap.set_capture_mode(match profile.capture_mode {
            hotwire_profile::CaptureMode::Capture => hotwire_input_macos::CaptureMode::Capture,
            hotwire_profile::CaptureMode::ModifiedCapture => {
                hotwire_input_macos::CaptureMode::Capture
            }
            hotwire_profile::CaptureMode::Passthrough => {
                hotwire_input_macos::CaptureMode::Passthrough
            }
        });
    }

    /// Routes one event off the Quartz callback thread and publishes all
    /// resulting adapter receipts to the live board.
    pub async fn route_event(&self, app: &tauri::AppHandle, event: hotwire_core::PhysicalKeyEvent) {
        let outcome = {
            let mut router = self.router.lock().expect("router lock");
            router.on_event(&event)
        };

        for invocation in outcome.invocations {
            let receipt = self
                .adapters
                .run_invocation(invocation, &event.physical_code)
                .await;
            self.record_and_emit(app, receipt);
        }
        for release in outcome.releases {
            let receipt = self
                .adapters
                .release(
                    &release.adapter_id,
                    &release.execution_id,
                    &release.physical_code,
                )
                .await;
            self.record_and_emit(app, receipt);
        }
    }

    fn record_and_emit(&self, app: &tauri::AppHandle, receipt: ActionReceipt) {
        if let Ok(mut last) = self.last_receipt.lock() {
            *last = Some(receipt.clone());
        }
        let _ = crate::events::emit_action_receipt(app, &receipt);
    }

    /// Whether the shell is currently paused (its adapter holds released).
    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.adapters.is_paused()
    }

    /// Pauses the shell: stops capture on the tap and cancels/releases every
    /// active adapter hold. Idempotent — a second pause releases nothing.
    ///
    /// Returns how many adapter holds were released.
    pub async fn pause(&self) -> usize {
        let released = pause_recovery(&self.tap, &self.adapters, &self.recovery).await;
        self.router.lock().expect("router lock").reset();
        sync_pause_label(&self.pause_item, self.tap.is_paused());
        released
    }

    /// Resumes the shell after a pause (adapter holds stay released).
    ///
    /// Returns the new paused state (`false` when resumed).
    pub async fn resume(&self) -> bool {
        let paused = resume_recovery(&self.tap, &self.adapters, &self.recovery).await;
        sync_pause_label(&self.pause_item, paused);
        paused
    }

    /// Shuts the shell down: stops capture (releasing any injected keys) and
    /// cancels/releases every active adapter hold. Idempotent.
    ///
    /// Returns how many adapter holds were released.
    pub async fn shutdown(&self) -> usize {
        let released = shutdown_recovery(&self.tap, &self.adapters, &self.recovery).await;
        self.router.lock().expect("router lock").reset();
        released
    }
}

/// Pauses recovery across both surfaces, testable without a window or menu.
///
/// The whole operation — stop capture on the tap, then gate the adapter
/// surface and release every active hold — runs behind `gate`, so a concurrent
/// resume/shutdown can never interleave the two surfaces. Fail-open ordering:
/// capture is paused before any adapter hold is released. The adapter surface
/// waits for any in-progress start first. Idempotent via the tracking map: a
/// second pause releases nothing, but retries holds whose release failed.
pub async fn pause_recovery(
    tap: &QuartzEventTap,
    adapters: &AdapterState,
    gate: &RecoveryGate,
) -> usize {
    let _gate = gate.lock().await;
    pause_capture(tap);
    adapters.pause().await
}

/// Resumes recovery on the tap, re-enabling adapter starts before capture.
///
/// The whole operation runs behind `gate` so a concurrent shutdown cannot stop
/// the tap between the `is_stopped()` check and `resume_capture`. A stopped
/// shell stays stopped — nothing re-enables starts after shutdown.
pub async fn resume_recovery(
    tap: &QuartzEventTap,
    adapters: &AdapterState,
    gate: &RecoveryGate,
) -> bool {
    let _gate = gate.lock().await;
    if adapters.is_stopped() {
        return tap.is_paused();
    }
    adapters.resume().await;
    resume_capture(tap)
}

/// Shutdown recovery across both surfaces, testable without a window or menu.
///
/// The whole operation runs behind `gate` so no stale resume can restart
/// capture mid-shutdown. Fail-open ordering: the tap is stopped (which
/// releases any keys it injected) and the emergency pause is engaged before
/// active adapter holds are cancelled, so a stopped shell is never left with
/// capture un-paused. Adapter starts are disabled permanently and every active
/// hold is released, with in-progress starts waited out first. Idempotent via
/// the tracking map.
pub async fn shutdown_recovery(
    tap: &QuartzEventTap,
    adapters: &AdapterState,
    gate: &RecoveryGate,
) -> usize {
    let _gate = gate.lock().await;
    tap.stop();
    tap.emergency_pause();
    adapters.shutdown().await
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
        let gate = RecoveryGate::default();
        let started = start_hold(&adapters).await;
        assert_eq!(started.status, hotwire_core::ActionStatus::Started);
        assert_eq!(adapters.active_count(), 1);

        // Pause stops capture and releases the hold, key before modifiers.
        assert!(!tap.is_paused());
        assert_eq!(pause_recovery(&tap, &adapters, &gate).await, 1);
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
        assert_eq!(pause_recovery(&tap, &adapters, &gate).await, 0);
        assert_eq!(
            recorded.ups.lock().expect("lock").len(),
            2,
            "no duplicate ups"
        );
    }

    #[tokio::test]
    async fn shutdown_releases_active_holds_and_is_idempotent() {
        let (tap, adapters, recorded) = recovery_fixture();
        let gate = RecoveryGate::default();
        let started = start_hold(&adapters).await;
        assert_eq!(started.status, hotwire_core::ActionStatus::Started);
        assert_eq!(adapters.active_count(), 1);

        assert_eq!(shutdown_recovery(&tap, &adapters, &gate).await, 1);
        assert_eq!(tap.status(), hotwire_input_macos::TapStatus::Stopped);
        assert!(tap.is_paused(), "shutdown engages the fail-open pause");
        assert_eq!(
            adapters.active_count(),
            0,
            "no adapter hold survives shutdown"
        );
        assert_eq!(recorded.downs.lock().expect("lock").len(), 2);
        assert_eq!(recorded.ups.lock().expect("lock").len(), 2, "balanced ups");
        assert_eq!(recorded.ups.lock().expect("lock").as_slice(), &[0x31, 0x3F]);

        // A second shutdown releases nothing.
        assert_eq!(shutdown_recovery(&tap, &adapters, &gate).await, 0);
        assert_eq!(
            recorded.ups.lock().expect("lock").len(),
            2,
            "no duplicate ups"
        );
    }

    #[tokio::test]
    async fn shutdown_after_pause_is_a_noop() {
        let (tap, adapters, recorded) = recovery_fixture();
        let gate = RecoveryGate::default();
        start_hold(&adapters).await;
        assert_eq!(pause_recovery(&tap, &adapters, &gate).await, 1);

        // A later shutdown must not double-release the already-freed holds.
        assert_eq!(shutdown_recovery(&tap, &adapters, &gate).await, 0);
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

    #[tokio::test]
    async fn lifecycle_operations_are_serialized_by_the_recovery_gate() {
        // While the recovery gate is held, a lifecycle operation must wait —
        // the mechanism that keeps a resume from reading `is_stopped()` and
        // then restarting capture while a shutdown is still running.
        let (tap, adapters, _) = recovery_fixture();
        let gate = Arc::new(RecoveryGate::default());
        let tap = Arc::new(tap);
        let adapters = Arc::new(adapters);

        let held = gate.lock().await;
        let resume = {
            let (tap, adapters, gate) =
                (Arc::clone(&tap), Arc::clone(&adapters), Arc::clone(&gate));
            tokio::spawn(async move { resume_recovery(&tap, &adapters, &gate).await })
        };
        tokio::task::yield_now().await;
        assert!(
            !resume.is_finished(),
            "resume must wait behind the recovery gate"
        );

        drop(held);
        resume
            .await
            .expect("resume completes after the gate is released");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_resume_and_shutdown_never_leave_capture_unpaused() {
        // A resume racing a shutdown used to be able to read `is_stopped()` as
        // false and then restart capture after shutdown had stopped the shell.
        // Serializing each complete lifecycle operation behind the recovery
        // gate makes the final state deterministic: adapter stopped and
        // capture off, however the race interleaves.
        for _ in 0..30 {
            let (tap, adapters, _) = recovery_fixture();
            let gate = Arc::new(RecoveryGate::default());
            let tap = Arc::new(tap);
            let adapters = Arc::new(adapters);
            pause_recovery(&tap, &adapters, &gate).await;

            let barrier = Arc::new(tokio::sync::Barrier::new(3));
            let resume = {
                let (tap, adapters, gate, barrier) = (
                    Arc::clone(&tap),
                    Arc::clone(&adapters),
                    Arc::clone(&gate),
                    Arc::clone(&barrier),
                );
                tokio::spawn(async move {
                    barrier.wait().await;
                    resume_recovery(&tap, &adapters, &gate).await
                })
            };
            let shutdown = {
                let (tap, adapters, gate, barrier) = (
                    Arc::clone(&tap),
                    Arc::clone(&adapters),
                    Arc::clone(&gate),
                    Arc::clone(&barrier),
                );
                tokio::spawn(async move {
                    barrier.wait().await;
                    shutdown_recovery(&tap, &adapters, &gate).await
                })
            };
            barrier.wait().await;
            let _ = tokio::join!(resume, shutdown);

            assert!(adapters.is_stopped(), "shutdown always wins the shell");
            assert_eq!(
                tap.status(),
                hotwire_input_macos::TapStatus::Stopped,
                "capture is not running after shutdown"
            );
            assert!(
                tap.is_paused(),
                "a stale resume must not leave capture un-paused"
            );
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_pause_and_resume_keep_tap_and_adapter_in_agreement() {
        // Pause and resume must never diverge the adapter acceptance state from
        // the tap. Whichever operation completes last wins both surfaces, so
        // the shell's paused state and the tap's pause flag always agree.
        for _ in 0..30 {
            let (tap, adapters, _) = recovery_fixture();
            let gate = Arc::new(RecoveryGate::default());
            let tap = Arc::new(tap);
            let adapters = Arc::new(adapters);

            let barrier = Arc::new(tokio::sync::Barrier::new(3));
            let pause = {
                let (tap, adapters, gate, barrier) = (
                    Arc::clone(&tap),
                    Arc::clone(&adapters),
                    Arc::clone(&gate),
                    Arc::clone(&barrier),
                );
                tokio::spawn(async move {
                    barrier.wait().await;
                    pause_recovery(&tap, &adapters, &gate).await
                })
            };
            let resume = {
                let (tap, adapters, gate, barrier) = (
                    Arc::clone(&tap),
                    Arc::clone(&adapters),
                    Arc::clone(&gate),
                    Arc::clone(&barrier),
                );
                tokio::spawn(async move {
                    barrier.wait().await;
                    resume_recovery(&tap, &adapters, &gate).await
                })
            };
            barrier.wait().await;
            let _ = tokio::join!(pause, resume);

            assert_eq!(
                adapters.is_paused(),
                tap.is_paused(),
                "the adapter acceptance state and the tap must agree"
            );
        }
    }
}
