//! Shell-side adapter registry and receipt plumbing.
//!
//! This is the vertical-slice bridge between the typed IPC commands and the
//! first-party adapters (ADP-001): it registers Herdr and Papegøye, runs
//! invocations, ends and cancels hold executions, and turns every outcome into
//! an [`ActionReceipt`] that the shell broadcasts to the UI. All OS-side work
//! happens inside the adapters behind their platform seams.
//!
//! Recovery coordination (spec §15.5) lives here too. A single async
//! [`AdapterState::ops`] gate serializes every state transition — run,
//! release, cancel, release-all, and the pause/shutdown lifecycle — so a
//! recovery cannot snapshot `active` while a start is mid-injection and miss a
//! hold that lands just after. While [`AdapterLifecycle::paused`] or
//! [`AdapterLifecycle::stopped`] is set, [`AdapterState::run`] rejects fresh
//! work instead of creating a held key. An invocation stays tracked until its
//! release/cancel succeeds, so a failed release is retried on the next
//! recovery pass instead of being dropped while the key is still logically
//! held.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use hotwire_adapter_comet::CometAdapter;
use hotwire_adapter_herdr::{default_platform as herdr_platform, HerdrAdapter};
use hotwire_adapter_papegoye::{default_platform as papegoye_platform, PapegoyeAdapter};
use hotwire_adapter_sdk::{
    ActionInvocation, ActionResult, DetectionResult, ExecutionContext, ValidationResult,
};
use hotwire_adapter_tools::ToolAdapter;
use hotwire_core::{ActionReceipt, ActionStatus, Trigger};
use hotwire_router::AdapterRegistry;
use serde_json::Value;

/// The lifecycle flags that gate new adapter starts during recovery.
///
/// `paused` stops fresh starts while the shell unwinds (and is cleared again
/// on resume); `stopped` is set by shutdown and never cleared.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct AdapterLifecycle {
    paused: bool,
    stopped: bool,
}

/// The adapters the shell owns, together with the state needed to end or
/// cancel in-flight executions from the UI.
pub struct AdapterState {
    registry: AdapterRegistry,
    /// execution id → the invocation currently in flight.
    active: Mutex<HashMap<String, ActionInvocation>>,
    next_execution: AtomicU64,
    /// Serializes every adapter state transition so pause/shutdown cannot race
    /// a start: recovery waits for an in-progress start to settle, then
    /// releases everything, and no new start can slip in behind it.
    ops: tokio::sync::Mutex<()>,
    /// The paused/stopped flags consulted by [`AdapterState::run`].
    lifecycle: Mutex<AdapterLifecycle>,
}

impl AdapterState {
    /// Builds a registry with the first-party Herdr and Papegøye adapters.
    #[must_use]
    pub fn new() -> Self {
        let mut registry = AdapterRegistry::new();
        registry
            .register(Arc::new(HerdrAdapter::new(herdr_platform())))
            .expect("herdr adapter registers exactly once");
        registry
            .register(Arc::new(CometAdapter::new()))
            .expect("comet adapter registers exactly once");
        for (id, name, capabilities) in [
            ("claude-code", "Claude Code", &["launch", "prompt"][..]),
            ("codex", "Codex", &["launch", "prompt"][..]),
            ("terminal", "Terminal", &["open", "run"][..]),
            ("git", "Git", &["diff", "commit", "pr"][..]),
            ("app", "Application", &["open_or_focus"][..]),
            ("shortcut", "Shortcut", &["send"][..]),
        ] {
            registry
                .register(Arc::new(ToolAdapter::new(id, name, capabilities)))
                .expect("tool adapter registers exactly once");
        }
        registry
            .register(Arc::new(PapegoyeAdapter::new(papegoye_platform())))
            .expect("papegoye adapter registers exactly once");
        Self::with_registry(registry)
    }

    /// Builds a state around a pre-populated registry (used by tests and the
    /// recovery lifecycle).
    pub(crate) fn with_registry(registry: AdapterRegistry) -> Self {
        Self {
            registry,
            active: Mutex::new(HashMap::new()),
            next_execution: AtomicU64::new(0),
            ops: tokio::sync::Mutex::new(()),
            lifecycle: Mutex::new(AdapterLifecycle::default()),
        }
    }

    /// How many adapter executions are currently tracked as in flight.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn active_count(&self) -> usize {
        self.active.lock().expect("active lock").len()
    }

    /// Whether adapter starts are paused (recovery in progress).
    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.lifecycle.lock().expect("lifecycle lock").paused
    }

    /// Whether the shell has been shut down (adapter starts disabled forever).
    #[must_use]
    pub fn is_stopped(&self) -> bool {
        self.lifecycle.lock().expect("lifecycle lock").stopped
    }

    /// Runs one action through its adapter and returns the resulting receipt.
    ///
    /// The execution is serialized behind the operation gate: a pause/shutdown
    /// waits for it to settle (or roll back) before releasing holds, and fresh
    /// work is rejected while paused or stopped so recovery can never race a
    /// start into a held key. A `hold` execution that reports `Started` stays
    /// tracked so the UI can later end it with [`AdapterState::release`] or
    /// [`AdapterState::cancel`].
    pub async fn run(
        &self,
        adapter_id: &str,
        action_id: &str,
        trigger: Trigger,
        config: Value,
        physical_code: &str,
    ) -> ActionReceipt {
        let invocation = ActionInvocation {
            execution_id: self.next_execution_id(),
            action_id: action_id.to_string(),
            adapter_id: adapter_id.to_string(),
            profile_id: "shell".to_string(),
            binding_id: "shell".to_string(),
            trigger,
            config,
            context: ExecutionContext {
                active_application: None,
                cwd: None,
                profile_id: "shell".to_string(),
                binding_id: "shell".to_string(),
                trigger,
                timestamp: now_nanos(),
            },
        };
        self.run_invocation(invocation, physical_code).await
    }

    /// Runs a router-produced invocation while preserving its profile,
    /// binding, and execution identity.
    pub async fn run_invocation(
        &self,
        invocation: ActionInvocation,
        physical_code: &str,
    ) -> ActionReceipt {
        let _gate = self.ops.lock().await;
        {
            let lifecycle = self.lifecycle.lock().expect("lifecycle lock");
            if lifecycle.paused || lifecycle.stopped {
                return failed_receipt(
                    &invocation.adapter_id,
                    &invocation.execution_id,
                    physical_code,
                    if lifecycle.stopped {
                        "adapter starts are disabled after shutdown".to_string()
                    } else {
                        "adapter starts are paused while recovery runs".to_string()
                    },
                );
            }
        }
        let result = self.registry.execute(&invocation).await;
        if result.status == ActionStatus::Started {
            self.active
                .lock()
                .expect("active lock")
                .insert(invocation.execution_id.clone(), invocation.clone());
        }
        build_receipt(&invocation, physical_code, &result)
    }

    /// Ends a tracked hold execution and returns its completion receipt.
    pub async fn release(
        &self,
        adapter_id: &str,
        execution_id: &str,
        physical_code: &str,
    ) -> ActionReceipt {
        let _gate = self.ops.lock().await;
        self.finish_tracked(
            adapter_id,
            execution_id,
            physical_code,
            ActionStatus::Succeeded,
            |_| self.registry.release(adapter_id, execution_id),
        )
        .await
    }

    /// Cancels a tracked execution and returns its `Cancelled` receipt.
    pub async fn cancel(
        &self,
        adapter_id: &str,
        execution_id: &str,
        physical_code: &str,
    ) -> ActionReceipt {
        let _gate = self.ops.lock().await;
        self.finish_tracked(
            adapter_id,
            execution_id,
            physical_code,
            ActionStatus::Cancelled,
            |_| self.registry.cancel(adapter_id, execution_id),
        )
        .await
    }

    /// Ends every tracked execution (used on shutdown).
    ///
    /// Serialized behind the operation gate so a snapshot can never miss a
    /// hold that a start is still injecting. Successfully released executions
    /// are removed from the tracking map; failed ones stay tracked so a later
    /// recovery pass retries them without duplicating the successful key-ups.
    #[cfg_attr(not(test), allow(dead_code))]
    pub async fn release_active(&self) -> usize {
        let _gate = self.ops.lock().await;
        self.release_active_locked().await
    }

    /// Ends every tracked execution while already holding the operation gate.
    async fn release_active_locked(&self) -> usize {
        let ids: Vec<(String, String)> = self
            .active
            .lock()
            .expect("active lock")
            .iter()
            .map(|(id, invocation)| (id.clone(), invocation.adapter_id.clone()))
            .collect();
        let mut released = 0;
        for (execution_id, adapter_id) in ids {
            if self
                .registry
                .release(&adapter_id, &execution_id)
                .await
                .is_ok()
            {
                self.active
                    .lock()
                    .expect("active lock")
                    .remove(&execution_id);
                released += 1;
            }
        }
        released
    }

    /// Pauses recovery: disables fresh starts and releases every active hold.
    ///
    /// The operation gate makes this wait for any in-progress start to settle
    /// first, so the hold it just created is released too. Repeated calls
    /// release nothing further — successful holds are gone from the tracking
    /// map — but a hold whose release previously failed stays tracked and is
    /// retried.
    ///
    /// Returns how many adapter holds were released.
    pub async fn pause(&self) -> usize {
        let _gate = self.ops.lock().await;
        self.lifecycle.lock().expect("lifecycle lock").paused = true;
        self.release_active_locked().await
    }

    /// Resumes after a pause: re-enables fresh adapter starts.
    ///
    /// A stopped shell stays stopped — nothing re-enables starts after
    /// shutdown.
    pub async fn resume(&self) {
        let _gate = self.ops.lock().await;
        let mut lifecycle = self.lifecycle.lock().expect("lifecycle lock");
        if !lifecycle.stopped {
            lifecycle.paused = false;
        }
    }

    /// Shuts the adapters down: disables starts permanently and releases every
    /// active hold. Idempotent and retry-safe, like [`AdapterState::pause`].
    ///
    /// Returns how many adapter holds were released.
    pub async fn shutdown(&self) -> usize {
        let _gate = self.ops.lock().await;
        {
            let mut lifecycle = self.lifecycle.lock().expect("lifecycle lock");
            lifecycle.stopped = true;
            lifecycle.paused = true;
        }
        self.release_active_locked().await
    }

    /// Probes one registered adapter for machine-level presence.
    pub async fn detect(&self, adapter_id: &str) -> Result<DetectionResult, String> {
        let adapter = self
            .registry
            .get(adapter_id)
            .ok_or_else(|| format!("adapter `{adapter_id}` is not registered"))?;
        Ok(adapter.detect().await)
    }

    /// Validates a binding config against one registered adapter.
    pub async fn validate_config(
        &self,
        adapter_id: &str,
        config: &Value,
    ) -> Result<ValidationResult, String> {
        let adapter = self
            .registry
            .get(adapter_id)
            .ok_or_else(|| format!("adapter `{adapter_id}` is not registered"))?;
        Ok(adapter.validate(config).await)
    }

    /// Ends or cancels a tracked execution, publishing its completion receipt.
    ///
    /// `success_status` is the semantic outcome of a successful finish: a
    /// `release` completes as `Succeeded`, a `cancel` as `Cancelled`.
    ///
    /// The invocation is only removed from the tracking map once its
    /// release/cancel succeeds. A failed finish leaves it tracked so the next
    /// recovery pass can retry it — the key is still logically held, so
    /// dropping it here would strand it.
    async fn finish_tracked<F, Fut>(
        &self,
        adapter_id: &str,
        execution_id: &str,
        physical_code: &str,
        success_status: ActionStatus,
        finish: F,
    ) -> ActionReceipt
    where
        F: Fn(&ActionInvocation) -> Fut,
        Fut: std::future::Future<Output = Result<(), hotwire_adapter_sdk::AdapterError>>,
    {
        let Some(invocation) = self
            .active
            .lock()
            .expect("active lock")
            .get(execution_id)
            .cloned()
        else {
            return failed_receipt(
                adapter_id,
                execution_id,
                physical_code,
                format!("no active execution `{execution_id}` to end"),
            );
        };
        let result = match finish(&invocation).await {
            Ok(()) => {
                self.active
                    .lock()
                    .expect("active lock")
                    .remove(execution_id);
                ActionResult {
                    execution_id: execution_id.to_string(),
                    status: success_status,
                    message: None,
                }
            }
            Err(error) => ActionResult {
                execution_id: execution_id.to_string(),
                status: ActionStatus::Failed,
                message: Some(error.to_string()),
            },
        };
        build_receipt(&invocation, physical_code, &result)
    }

    fn next_execution_id(&self) -> String {
        format!(
            "shell-exec-{}",
            self.next_execution.fetch_add(1, Ordering::Relaxed)
        )
    }
}

impl Default for AdapterState {
    fn default() -> Self {
        Self::new()
    }
}

/// Turns an invocation and its result into a receipt the live board can show.
#[must_use]
pub fn build_receipt(
    invocation: &ActionInvocation,
    physical_code: &str,
    result: &ActionResult,
) -> ActionReceipt {
    ActionReceipt {
        execution_id: invocation.execution_id.clone(),
        profile_id: invocation.profile_id.clone(),
        binding_id: invocation.binding_id.clone(),
        physical_code: physical_code.to_string(),
        action_id: invocation.action_id.clone(),
        adapter_id: invocation.adapter_id.clone(),
        status: result.status.clone(),
        message: result.message.clone(),
    }
}

/// A `Failed` receipt for an untracked execution, so the UI always sees an
/// explicit outcome instead of a silently dropped action.
#[must_use]
pub fn failed_receipt(
    adapter_id: &str,
    execution_id: &str,
    physical_code: &str,
    message: String,
) -> ActionReceipt {
    ActionReceipt {
        execution_id: execution_id.to_string(),
        profile_id: "shell".to_string(),
        binding_id: "shell".to_string(),
        physical_code: physical_code.to_string(),
        action_id: String::new(),
        adapter_id: adapter_id.to_string(),
        status: ActionStatus::Failed,
        message: Some(message),
    }
}

fn now_nanos() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use hotwire_adapter_sdk::{Adapter, AdapterError, AdapterManifest};
    use serde_json::json;
    use std::sync::atomic::AtomicBool;
    use tokio::sync::oneshot;

    fn invocation(execution_id: &str) -> ActionInvocation {
        ActionInvocation {
            execution_id: execution_id.into(),
            action_id: "app.open_or_focus".into(),
            adapter_id: "test".into(),
            profile_id: "p".into(),
            binding_id: "b".into(),
            trigger: Trigger::Press,
            config: json!({}),
            context: ExecutionContext {
                active_application: None,
                cwd: None,
                profile_id: "p".into(),
                binding_id: "b".into(),
                trigger: Trigger::Press,
                timestamp: "0".into(),
            },
        }
    }

    #[test]
    fn receipt_carries_the_full_route_context() {
        let invocation = invocation("exec-1");
        let result = ActionResult {
            execution_id: "exec-1".into(),
            status: ActionStatus::Succeeded,
            message: Some("Focused Herdr".into()),
        };

        let receipt = build_receipt(&invocation, "Numpad5", &result);
        assert_eq!(receipt.physical_code, "Numpad5");
        assert_eq!(receipt.action_id, "app.open_or_focus");
        assert_eq!(receipt.adapter_id, "test");
        assert_eq!(receipt.status, ActionStatus::Succeeded);
        assert_eq!(receipt.message.as_deref(), Some("Focused Herdr"));
    }

    #[test]
    fn failed_receipt_is_explicit_about_untracked_executions() {
        let receipt = failed_receipt("papegoye", "exec-9", "Numpad0", "gone".into());
        assert_eq!(receipt.status, ActionStatus::Failed);
        assert!(receipt
            .message
            .as_deref()
            .is_some_and(|m| m.contains("gone")));
    }

    #[tokio::test]
    async fn release_of_an_untracked_execution_yields_a_failed_receipt() {
        let state = AdapterState::new();
        let receipt = state.release("papegoye", "exec-9", "Numpad0").await;
        assert_eq!(receipt.status, ActionStatus::Failed);
        assert!(receipt
            .message
            .as_deref()
            .is_some_and(|message| message.contains("no active execution")));
    }

    #[tokio::test]
    async fn detect_and_validate_reach_the_registered_adapters() {
        let state = AdapterState::new();

        let validated = state
            .validate_config("herdr", &json!({ "bundleId": "dev.herdr.app" }))
            .await
            .expect("herdr is registered");
        assert!(validated.valid);

        assert!(state.detect("missing").await.is_err());
    }

    #[tokio::test]
    async fn run_reports_an_unknown_adapter_as_a_failed_receipt() {
        let state = AdapterState::new();
        let receipt = state
            .run(
                "missing",
                "app.open_or_focus",
                Trigger::Press,
                json!({}),
                "Numpad5",
            )
            .await;
        assert_eq!(receipt.status, ActionStatus::Failed);
        assert!(receipt
            .message
            .as_deref()
            .is_some_and(|message| message.contains("not registered")));
    }

    /// A hold adapter that stays `Started` so the shell tracks it, and records
    /// every cancel/release. Used to exercise receipt semantics without
    /// touching any real OS side effects.
    struct TrackingAdapter {
        manifest: AdapterManifest,
        cancelled: Mutex<Vec<String>>,
        released: Mutex<Vec<String>>,
        fail_release: AtomicBool,
    }

    impl TrackingAdapter {
        fn new() -> Self {
            Self {
                manifest: AdapterManifest {
                    id: "track".into(),
                    name: "Track".into(),
                    version: "0.1.0".into(),
                    icon: "track".into(),
                    capabilities: vec!["start".into(), "stop".into(), "cancel".into()],
                    config_schema: json!({}),
                },
                cancelled: Mutex::new(Vec::new()),
                released: Mutex::new(Vec::new()),
                fail_release: AtomicBool::new(false),
            }
        }

        fn with_failing_release(self) -> Self {
            self.fail_release.store(true, Ordering::SeqCst);
            self
        }
    }

    #[async_trait]
    impl Adapter for TrackingAdapter {
        fn manifest(&self) -> &AdapterManifest {
            &self.manifest
        }

        async fn detect(&self) -> DetectionResult {
            DetectionResult {
                id: self.manifest.id.clone(),
                detected: false,
                version: None,
                path: None,
            }
        }

        async fn validate(&self, _config: &Value) -> ValidationResult {
            ValidationResult {
                valid: true,
                errors: Vec::new(),
            }
        }

        async fn execute(&self, invocation: &ActionInvocation) -> ActionResult {
            ActionResult {
                execution_id: invocation.execution_id.clone(),
                status: ActionStatus::Started,
                message: Some("tracked".into()),
            }
        }

        async fn cancel(&self, execution_id: &str) -> Result<(), AdapterError> {
            self.cancelled
                .lock()
                .expect("lock")
                .push(execution_id.to_string());
            Ok(())
        }

        async fn release(&self, execution_id: &str) -> Result<(), AdapterError> {
            self.released
                .lock()
                .expect("lock")
                .push(execution_id.to_string());
            if self.fail_release.load(Ordering::SeqCst) {
                Err(AdapterError::Other("release failed".into()))
            } else {
                Ok(())
            }
        }
    }

    /// A hold adapter whose `execute` can be gated behind a oneshot so a start
    /// can be parked mid-execution while recovery runs. Records downs/ups and
    /// can fail releases on demand, so the pause/shutdown race is reproduced
    /// deterministically.
    struct GateAdapter {
        manifest: AdapterManifest,
        downs: Mutex<Vec<u16>>,
        ups: Mutex<Vec<u16>>,
        /// Whether `execute` blocks on `unblock` after signalling `entered`.
        block: AtomicBool,
        entered: Mutex<Option<oneshot::Sender<()>>>,
        unblock: Mutex<Option<oneshot::Receiver<()>>>,
        fail_release: AtomicBool,
    }

    #[async_trait]
    impl Adapter for GateAdapter {
        fn manifest(&self) -> &AdapterManifest {
            &self.manifest
        }

        async fn detect(&self) -> DetectionResult {
            DetectionResult {
                id: self.manifest.id.clone(),
                detected: false,
                version: None,
                path: None,
            }
        }

        async fn validate(&self, _config: &Value) -> ValidationResult {
            ValidationResult {
                valid: true,
                errors: Vec::new(),
            }
        }

        async fn execute(&self, invocation: &ActionInvocation) -> ActionResult {
            if self.block.load(Ordering::SeqCst) {
                if let Some(entered) = self.entered.lock().expect("entered lock").take() {
                    let _ = entered.send(());
                }
                let unblock = self.unblock.lock().expect("unblock lock").take();
                if let Some(unblock) = unblock {
                    let _ = unblock.await;
                }
            }
            self.downs.lock().expect("downs lock").push(0x31);
            ActionResult {
                execution_id: invocation.execution_id.clone(),
                status: ActionStatus::Started,
                message: Some("gated hold".into()),
            }
        }

        async fn cancel(&self, _execution_id: &str) -> Result<(), AdapterError> {
            if self.fail_release.load(Ordering::SeqCst) {
                return Err(AdapterError::Other("release failed".into()));
            }
            self.ups.lock().expect("ups lock").push(0x31);
            Ok(())
        }

        async fn release(&self, execution_id: &str) -> Result<(), AdapterError> {
            self.cancel(execution_id).await
        }
    }

    fn gate_adapter_manifest() -> AdapterManifest {
        AdapterManifest {
            id: "gate".into(),
            name: "Gate".into(),
            version: "0.1.0".into(),
            icon: "gate".into(),
            capabilities: vec!["start".into(), "stop".into(), "cancel".into()],
            config_schema: json!({}),
        }
    }

    /// A racing fixture: `execute` blocks on a oneshot the test releases.
    ///
    /// Returns the state, the adapter, a receiver fired when the start reaches
    /// the adapter, and the sender that unblocks it.
    fn racing_state() -> (
        Arc<AdapterState>,
        Arc<GateAdapter>,
        oneshot::Receiver<()>,
        oneshot::Sender<()>,
    ) {
        let (entered_tx, entered_rx) = oneshot::channel();
        let (unblock_tx, unblock_rx) = oneshot::channel();
        let adapter = Arc::new(GateAdapter {
            manifest: gate_adapter_manifest(),
            downs: Mutex::new(Vec::new()),
            ups: Mutex::new(Vec::new()),
            block: AtomicBool::new(true),
            entered: Mutex::new(Some(entered_tx)),
            unblock: Mutex::new(Some(unblock_rx)),
            fail_release: AtomicBool::new(false),
        });
        let mut registry = AdapterRegistry::new();
        registry
            .register(Arc::clone(&adapter) as Arc<dyn Adapter>)
            .expect("registers");
        (
            Arc::new(AdapterState::with_registry(registry)),
            adapter,
            entered_rx,
            unblock_tx,
        )
    }

    /// A plain fixture whose `execute` never blocks (used for rejection and
    /// retry tests).
    fn plain_state() -> (Arc<AdapterState>, Arc<GateAdapter>) {
        let adapter = Arc::new(GateAdapter {
            manifest: gate_adapter_manifest(),
            downs: Mutex::new(Vec::new()),
            ups: Mutex::new(Vec::new()),
            block: AtomicBool::new(false),
            entered: Mutex::new(None),
            unblock: Mutex::new(None),
            fail_release: AtomicBool::new(false),
        });
        let mut registry = AdapterRegistry::new();
        registry
            .register(Arc::clone(&adapter) as Arc<dyn Adapter>)
            .expect("registers");
        (Arc::new(AdapterState::with_registry(registry)), adapter)
    }

    fn tracking_state() -> (AdapterState, Arc<TrackingAdapter>) {
        let adapter: Arc<TrackingAdapter> = Arc::new(TrackingAdapter::new());
        let mut registry = AdapterRegistry::new();
        registry
            .register(Arc::clone(&adapter) as Arc<dyn Adapter>)
            .expect("registers");
        (AdapterState::with_registry(registry), adapter)
    }

    #[tokio::test]
    async fn release_produces_a_succeeded_receipt_and_cancel_a_cancelled_one() {
        let (state, adapter) = tracking_state();

        let started = state
            .run("track", "voice.input", Trigger::Hold, json!({}), "Numpad0")
            .await;
        assert_eq!(started.status, ActionStatus::Started);
        let cancelled = state
            .cancel("track", &started.execution_id, "Numpad0")
            .await;
        assert_eq!(cancelled.status, ActionStatus::Cancelled);
        assert_eq!(
            adapter.cancelled.lock().expect("lock").as_slice(),
            std::slice::from_ref(&started.execution_id)
        );

        let started = state
            .run("track", "voice.input", Trigger::Hold, json!({}), "Numpad0")
            .await;
        let released = state
            .release("track", &started.execution_id, "Numpad0")
            .await;
        assert_eq!(released.status, ActionStatus::Succeeded);
        assert_eq!(
            adapter.released.lock().expect("lock").as_slice(),
            std::slice::from_ref(&started.execution_id)
        );
    }

    #[tokio::test]
    async fn a_failed_release_yields_a_failed_receipt() {
        let adapter: Arc<TrackingAdapter> = Arc::new(TrackingAdapter::new().with_failing_release());
        let mut registry = AdapterRegistry::new();
        registry
            .register(Arc::clone(&adapter) as Arc<dyn Adapter>)
            .expect("registers");
        let state = AdapterState::with_registry(registry);

        let started = state
            .run("track", "voice.input", Trigger::Hold, json!({}), "Numpad0")
            .await;
        let released = state
            .release("track", &started.execution_id, "Numpad0")
            .await;
        assert_eq!(released.status, ActionStatus::Failed);
    }

    #[tokio::test]
    async fn release_active_clears_successfully_released_executions() {
        let (state, _) = tracking_state();

        let first = state
            .run("track", "voice.input", Trigger::Hold, json!({}), "Numpad0")
            .await;
        let _second = state
            .run("track", "voice.input", Trigger::Hold, json!({}), "Numpad1")
            .await;

        assert_eq!(state.release_active().await, 2);

        // The tracking map now reflects reality: nothing is still in flight.
        let after = state.release("track", &first.execution_id, "Numpad0").await;
        assert_eq!(after.status, ActionStatus::Failed);
        assert!(after
            .message
            .as_deref()
            .is_some_and(|message| message.contains("no active execution")));
    }

    #[tokio::test]
    async fn pause_waits_for_a_blocked_start_then_releases_everything() {
        let (state, adapter, entered_rx, unblock_tx) = racing_state();

        let run = {
            let state = Arc::clone(&state);
            tokio::spawn(async move {
                state
                    .run("gate", "voice.input", Trigger::Hold, json!({}), "Numpad0")
                    .await
            })
        };
        entered_rx.await.expect("start reached the adapter");

        let pause = {
            let state = Arc::clone(&state);
            tokio::spawn(async move { state.pause().await })
        };
        tokio::task::yield_now().await;
        assert!(
            !pause.is_finished(),
            "pause must wait for the in-progress start to settle"
        );

        let _ = unblock_tx.send(());
        let started = run.await.expect("run task completed");
        assert_eq!(started.status, ActionStatus::Started);

        let released = pause.await.expect("pause task completed");
        assert_eq!(
            released, 1,
            "pause releases the hold that started while it waited"
        );
        assert_eq!(
            adapter.downs.lock().expect("downs lock").as_slice(),
            &[0x31]
        );
        assert_eq!(
            adapter.ups.lock().expect("ups lock").as_slice(),
            &[0x31],
            "the hold is released after the start settles"
        );
        assert_eq!(state.active_count(), 0, "no hold survives pause");
    }

    #[tokio::test]
    async fn shutdown_waits_for_a_blocked_start_then_releases_everything() {
        let (state, adapter, entered_rx, unblock_tx) = racing_state();

        let run = {
            let state = Arc::clone(&state);
            tokio::spawn(async move {
                state
                    .run("gate", "voice.input", Trigger::Hold, json!({}), "Numpad0")
                    .await
            })
        };
        entered_rx.await.expect("start reached the adapter");

        let shutdown = {
            let state = Arc::clone(&state);
            tokio::spawn(async move { state.shutdown().await })
        };
        tokio::task::yield_now().await;
        assert!(
            !shutdown.is_finished(),
            "shutdown must wait for the in-progress start to settle"
        );

        let _ = unblock_tx.send(());
        let started = run.await.expect("run task completed");
        assert_eq!(started.status, ActionStatus::Started);

        let released = shutdown.await.expect("shutdown task completed");
        assert_eq!(
            released, 1,
            "shutdown releases the hold that started while it waited"
        );
        assert_eq!(
            adapter.downs.lock().expect("downs lock").as_slice(),
            &[0x31]
        );
        assert_eq!(
            adapter.ups.lock().expect("ups lock").as_slice(),
            &[0x31],
            "the hold is released after the start settles"
        );
        assert_eq!(state.active_count(), 0);

        // Starts are disabled permanently after shutdown: nothing injected.
        let rejected = state
            .run("gate", "voice.input", Trigger::Hold, json!({}), "Numpad0")
            .await;
        assert_eq!(rejected.status, ActionStatus::Failed);
        assert_eq!(
            adapter.downs.lock().expect("downs lock").len(),
            1,
            "no new key down after shutdown"
        );
    }

    #[tokio::test]
    async fn run_is_rejected_while_paused_without_any_key_down() {
        let (state, adapter) = plain_state();

        assert_eq!(state.pause().await, 0, "nothing active at first pause");
        let receipt = state
            .run("gate", "voice.input", Trigger::Hold, json!({}), "Numpad0")
            .await;
        assert_eq!(receipt.status, ActionStatus::Failed);
        assert!(receipt
            .message
            .as_deref()
            .is_some_and(|message| message.contains("paused")));
        assert_eq!(state.active_count(), 0);
        assert!(
            adapter.downs.lock().expect("downs lock").is_empty(),
            "no key was injected while paused"
        );

        // Resume re-enables starts.
        state.resume().await;
        let started = state
            .run("gate", "voice.input", Trigger::Hold, json!({}), "Numpad0")
            .await;
        assert_eq!(started.status, ActionStatus::Started);
        assert_eq!(state.active_count(), 1);
    }

    #[tokio::test]
    async fn run_is_rejected_after_shutdown_without_any_key_down() {
        let (state, adapter) = plain_state();

        assert_eq!(state.shutdown().await, 0, "nothing active at shutdown");
        let receipt = state
            .run("gate", "voice.input", Trigger::Hold, json!({}), "Numpad0")
            .await;
        assert_eq!(receipt.status, ActionStatus::Failed);
        assert!(receipt
            .message
            .as_deref()
            .is_some_and(|message| message.contains("shutdown")));
        assert!(
            adapter.downs.lock().expect("downs lock").is_empty(),
            "no key was injected after shutdown"
        );

        // Nothing re-enables starts after shutdown, not even a resume.
        state.resume().await;
        let receipt = state
            .run("gate", "voice.input", Trigger::Hold, json!({}), "Numpad0")
            .await;
        assert_eq!(receipt.status, ActionStatus::Failed);
        assert!(
            adapter.downs.lock().expect("downs lock").is_empty(),
            "starts stay disabled after shutdown"
        );
    }

    #[tokio::test]
    async fn a_failed_release_stays_tracked_and_succeeds_on_recovery_retry() {
        let (state, adapter) = plain_state();

        let started = state
            .run("gate", "voice.input", Trigger::Hold, json!({}), "Numpad0")
            .await;
        assert_eq!(started.status, ActionStatus::Started);
        assert_eq!(state.active_count(), 1);

        adapter.fail_release.store(true, Ordering::SeqCst);
        let failed = state
            .release("gate", &started.execution_id, "Numpad0")
            .await;
        assert_eq!(failed.status, ActionStatus::Failed);
        assert_eq!(
            state.active_count(),
            1,
            "a failed release stays tracked so recovery can retry it"
        );

        adapter.fail_release.store(false, Ordering::SeqCst);
        assert_eq!(
            state.release_active().await,
            1,
            "recovery retries the failed hold"
        );
        assert_eq!(state.active_count(), 0);
        assert_eq!(
            adapter.downs.lock().expect("downs lock").as_slice(),
            &[0x31]
        );
        assert_eq!(
            adapter.ups.lock().expect("ups lock").as_slice(),
            &[0x31],
            "the retried release balances the held key"
        );
    }
}
