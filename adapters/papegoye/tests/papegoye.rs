//! Mocked integration tests for the Papegøye adapter.
//!
//! The platform is substituted with a recorder so the push-to-talk hold state
//! machine — start, release, cancel, shutdown — is exercised without touching a
//! real keyboard. These tests pin the spec invariants (spec §13.5): a down is
//! always paired with exactly one up, never repeated, and never left stuck.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use hotwire_adapter_papegoye::{PapegoyeAdapter, PapegoyeError, PapegoyePlatform};
use hotwire_adapter_sdk::{ActionInvocation, Adapter, AdapterError, ExecutionContext};
use hotwire_core::{ActionStatus, Trigger};
use serde_json::{json, Value};
use tokio::sync::oneshot;

/// Synthetic key events recorded by the mock platform.
#[derive(Default, Debug)]
struct MockEvents {
    downs: Vec<u16>,
    ups: Vec<u16>,
    held: Vec<u16>,
}

/// A scriptable Papegøye platform that records every injected key event.
///
/// `hold_down` is an optional gate: when set, the next `key_down` blocks on it
/// until the test releases it, so concurrent-execution races can be reproduced
/// deterministically. `fail_next_down` makes the next `key_down` fail (once),
/// so partial-injection failures can be reproduced.
struct MockPlatform {
    app_present: bool,
    events: Arc<Mutex<MockEvents>>,
    hold_down: Arc<Mutex<Option<oneshot::Receiver<()>>>>,
    fail_next_down: Arc<AtomicBool>,
}

impl MockPlatform {
    fn new() -> Self {
        Self {
            app_present: true,
            events: Arc::new(Mutex::new(MockEvents::default())),
            hold_down: Arc::new(Mutex::new(None)),
            fail_next_down: Arc::new(AtomicBool::new(false)),
        }
    }

    fn with_app_present(mut self, present: bool) -> Self {
        self.app_present = present;
        self
    }

    fn with_hold_down(self, receiver: oneshot::Receiver<()>) -> Self {
        *self.hold_down.lock().expect("hold down lock") = Some(receiver);
        self
    }
}

#[async_trait]
impl PapegoyePlatform for MockPlatform {
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
        self.app_present
    }

    async fn key_down(&self, keycode: u16) -> Result<(), PapegoyeError> {
        let gate = self.hold_down.lock().expect("hold down lock").take();
        if let Some(gate) = gate {
            let _ = gate.await;
        }
        if self.fail_next_down.swap(false, Ordering::SeqCst) {
            return Err(PapegoyeError::Inject("down failed".into()));
        }
        let mut events = self.events.lock().expect("events lock");
        events.downs.push(keycode);
        if !events.held.contains(&keycode) {
            events.held.push(keycode);
        }
        Ok(())
    }

    async fn key_up(&self, keycode: u16) -> Result<(), PapegoyeError> {
        let mut events = self.events.lock().expect("events lock");
        events.ups.push(keycode);
        events.held.retain(|code| *code != keycode);
        Ok(())
    }

    async fn release_all(&self) -> Vec<u16> {
        let mut events = self.events.lock().expect("events lock");
        let held = events.held.clone();
        for code in &held {
            events.ups.push(*code);
        }
        events.held.clear();
        held
    }
}

fn invocation(execution_id: &str, config: Value) -> ActionInvocation {
    invocation_with(execution_id, Trigger::Hold, config)
}

fn invocation_with(execution_id: &str, trigger: Trigger, config: Value) -> ActionInvocation {
    ActionInvocation {
        execution_id: execution_id.into(),
        action_id: "voice.input".into(),
        adapter_id: "papegoye".into(),
        profile_id: "p".into(),
        binding_id: "b".into(),
        trigger,
        config,
        context: ExecutionContext {
            active_application: None,
            cwd: None,
            profile_id: "p".into(),
            binding_id: "b".into(),
            trigger,
            timestamp: "0".into(),
        },
    }
}

fn adapter() -> (PapegoyeAdapter, Arc<Mutex<MockEvents>>) {
    let platform = MockPlatform::new();
    let events = platform.events.clone();
    (PapegoyeAdapter::new(Arc::new(platform)), events)
}

fn shortcut_config() -> Value {
    json!({ "shortcut": "fn+space" })
}

#[tokio::test]
async fn hold_posts_down_once_on_start_and_up_once_on_release() {
    let (adapter, events) = adapter();

    let started = adapter
        .execute(&invocation("exec-1", shortcut_config()))
        .await;
    assert_eq!(started.status, ActionStatus::Started);

    adapter.release("exec-1").await.expect("release succeeds");

    let events = events.lock().expect("events lock");
    assert_eq!(events.downs, vec![0x3F, 0x31], "each keycode down once");
    assert_eq!(events.ups, vec![0x31, 0x3F], "each keycode up once");
    assert_eq!(events.downs.len(), events.ups.len(), "no stuck keys");
}

#[tokio::test]
async fn executing_the_same_hold_twice_never_repeats_the_down() {
    let (adapter, events) = adapter();

    adapter
        .execute(&invocation("exec-1", shortcut_config()))
        .await;
    adapter
        .execute(&invocation("exec-1", shortcut_config()))
        .await;

    let events = events.lock().expect("events lock");
    assert_eq!(events.downs, vec![0x3F, 0x31], "repeated start is a no-op");
}

#[tokio::test]
async fn releasing_an_unknown_execution_errors() {
    let (adapter, _) = adapter();
    assert!(matches!(
        adapter.release("nope").await,
        Err(AdapterError::UnknownExecution(_))
    ));
}

#[tokio::test]
async fn cancel_releases_the_held_key_during_cancellation() {
    let (adapter, events) = adapter();

    adapter
        .execute(&invocation("exec-1", shortcut_config()))
        .await;
    adapter.cancel("exec-1").await.expect("cancel succeeds");

    {
        let events = events.lock().expect("events lock");
        assert_eq!(events.downs, vec![0x3F, 0x31]);
        assert_eq!(
            events.ups,
            vec![0x31, 0x3F],
            "release is attempted on cancel, primary key up first"
        );
        assert_eq!(events.downs.len(), events.ups.len());
    }

    assert!(matches!(
        adapter.cancel("exec-1").await,
        Err(AdapterError::UnknownExecution(_))
    ));
}

#[tokio::test]
async fn hold_cycles_leave_no_stuck_keys() {
    let (adapter, events) = adapter();

    for round in 0..5 {
        let id = format!("exec-{round}");
        adapter.execute(&invocation(&id, shortcut_config())).await;
        adapter.release(&id).await.expect("release succeeds");
    }

    let events = events.lock().expect("events lock");
    assert_eq!(
        events.downs.len(),
        events.ups.len(),
        "downs always pair with ups"
    );
    assert_eq!(events.downs.len(), 5 * 2, "one hold per round, no repeats");
}

#[tokio::test]
async fn overlapping_executions_release_shared_keys_exactly_once() {
    let (adapter, events) = adapter();

    // Two physical keys both bound to voice.input with the same push-to-talk
    // combo; holding the second must not post a repeated down, and the key
    // must stay down until the last holder releases.
    adapter
        .execute(&invocation("exec-1", shortcut_config()))
        .await;
    adapter
        .execute(&invocation("exec-2", shortcut_config()))
        .await;
    adapter.release("exec-1").await.expect("release succeeds");
    {
        let events = events.lock().expect("events lock");
        assert_eq!(events.downs, vec![0x3F, 0x31]);
        assert!(events.ups.is_empty(), "key still held by exec-2");
    }

    adapter.release("exec-2").await.expect("release succeeds");
    {
        let events = events.lock().expect("events lock");
        assert_eq!(events.ups, vec![0x31, 0x3F], "exactly one up per keycode");
        assert_eq!(events.downs.len(), events.ups.len());
    }
}

#[tokio::test]
async fn shutdown_release_all_ends_every_in_flight_hold() {
    let (adapter, events) = adapter();

    adapter
        .execute(&invocation("exec-1", shortcut_config()))
        .await;
    adapter
        .execute(&invocation("exec-2", shortcut_config()))
        .await;

    let mut released = adapter.release_all().await;
    released.sort_unstable();
    assert_eq!(released, vec![0x31, 0x3F], "each held key released once");

    {
        let events = events.lock().expect("events lock");
        assert_eq!(
            events.downs.len(),
            events.ups.len(),
            "no stuck keys on shutdown"
        );
    }

    // After shutdown nothing is active; releasing again errors.
    assert!(matches!(
        adapter.release("exec-1").await,
        Err(AdapterError::UnknownExecution(_))
    ));
}

#[tokio::test]
async fn press_trigger_sends_a_complete_tap() {
    let (adapter, events) = adapter();

    let result = adapter
        .execute(&invocation_with(
            "exec-1",
            Trigger::Press,
            shortcut_config(),
        ))
        .await;
    assert_eq!(result.status, ActionStatus::Succeeded);

    let events = events.lock().expect("events lock");
    assert_eq!(events.downs, vec![0x3F, 0x31]);
    assert_eq!(
        events.ups,
        vec![0x31, 0x3F],
        "tap completes down then up in reverse"
    );
}

#[tokio::test]
async fn double_press_trigger_is_rejected() {
    let (adapter, events) = adapter();

    let result = adapter
        .execute(&invocation_with(
            "exec-1",
            Trigger::DoublePress,
            shortcut_config(),
        ))
        .await;
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result
        .message
        .as_deref()
        .is_some_and(|message| message.contains("only supports `hold`")));

    let events = events.lock().expect("events lock");
    assert!(
        events.downs.is_empty(),
        "nothing injected for an unsupported trigger"
    );
}

#[tokio::test]
async fn detect_reflects_app_availability() {
    let present = PapegoyeAdapter::new(Arc::new(MockPlatform::new()));
    assert!(present.detect().await.detected);

    let absent = PapegoyeAdapter::new(Arc::new(MockPlatform::new().with_app_present(false)));
    assert!(!absent.detect().await.detected);
}

#[tokio::test]
async fn concurrent_starts_with_the_same_execution_id_reserve_once() {
    // Gate the first key-down so a second start runs while the first is parked
    // mid-hold. The reservation must happen before that await: otherwise both
    // starts observe the execution as absent and double-book the hold counts,
    // leaving the key logically held after a single release.
    let (release_tx, release_rx) = oneshot::channel();
    let platform = MockPlatform::new().with_hold_down(release_rx);
    let events = platform.events.clone();
    let adapter = Arc::new(PapegoyeAdapter::new(Arc::new(platform)));

    let first = {
        let adapter = Arc::clone(&adapter);
        let invocation = invocation("exec-1", shortcut_config());
        tokio::spawn(async move { adapter.execute(&invocation).await })
    };
    // Let the first task reach its blocked key-down await.
    tokio::task::yield_now().await;

    let second = {
        let adapter = Arc::clone(&adapter);
        let invocation = invocation("exec-1", shortcut_config());
        tokio::spawn(async move { adapter.execute(&invocation).await })
    };
    tokio::task::yield_now().await;

    release_tx.send(()).ok();

    let (first_result, second_result) = tokio::join!(first, second);
    assert_eq!(
        first_result.expect("first task ran").status,
        ActionStatus::Started
    );
    assert_eq!(
        second_result.expect("second task ran").status,
        ActionStatus::Started
    );

    // Exactly one hold was booked: a single release fully releases the key.
    adapter.release("exec-1").await.expect("release succeeds");
    let events = events.lock().expect("events lock");
    assert_eq!(events.downs, vec![0x3F, 0x31], "down posted once");
    assert_eq!(
        events.ups,
        vec![0x31, 0x3F],
        "no stuck key after one release"
    );
}

#[tokio::test]
async fn overlapping_starts_after_a_failed_injection_post_a_real_hold() {
    // exec-1 starts first, parks inside its gated key-down and then fails the
    // injection. exec-2 starts while exec-1 is parked. The 0→1 injection is
    // serialized, so exec-2 must not reserve during exec-1's partial state:
    // it waits, then posts a real hold itself. A phantom `Started` (reported
    // without any physical keys down) is the regression this guards against.
    let (release_tx, release_rx) = oneshot::channel();
    let platform = MockPlatform::new().with_hold_down(release_rx);
    platform.fail_next_down.store(true, Ordering::SeqCst);
    let events = platform.events.clone();
    let adapter = Arc::new(PapegoyeAdapter::new(Arc::new(platform)));

    let first = {
        let adapter = Arc::clone(&adapter);
        let invocation = invocation("exec-1", shortcut_config());
        tokio::spawn(async move { adapter.execute(&invocation).await })
    };
    // Let exec-1 reach its gated key-down while holding the injection lock.
    tokio::task::yield_now().await;

    let second = {
        let adapter = Arc::clone(&adapter);
        let invocation = invocation("exec-2", shortcut_config());
        tokio::spawn(async move { adapter.execute(&invocation).await })
    };
    tokio::task::yield_now().await;

    // exec-1's key-down now fails; it rolls back and releases the lock.
    release_tx.send(()).ok();

    let (first_result, second_result) = tokio::join!(first, second);
    assert_eq!(
        first_result.expect("first task ran").status,
        ActionStatus::Failed
    );
    let second = second_result.expect("second task ran");
    assert_eq!(
        second.status,
        ActionStatus::Started,
        "a later execution must post a real hold, never a phantom Started"
    );

    // exec-2 injected the full hold itself.
    {
        let events = events.lock().expect("events lock");
        assert_eq!(
            events.downs,
            vec![0x3F, 0x31],
            "the surviving hold is physically down"
        );
        assert!(events.ups.is_empty(), "nothing was left half-released");
    }

    // Releasing the surviving hold balances every down.
    adapter
        .release(&second.execution_id)
        .await
        .expect("release succeeds");
    let events = events.lock().expect("events lock");
    assert_eq!(events.downs.len(), events.ups.len(), "no stuck keys");
    assert_eq!(
        events.ups,
        vec![0x31, 0x3F],
        "key released before modifiers"
    );
}

#[tokio::test]
async fn release_waits_for_a_blocked_start_and_balances() {
    // A release arriving while exec-1's start is parked mid-injection must not
    // orphan the reservation: it waits behind the same operation lock, so the
    // start completes first and the release then removes the active entry and
    // balances the hold.
    let (release_tx, release_rx) = oneshot::channel();
    let platform = MockPlatform::new().with_hold_down(release_rx);
    let events = platform.events.clone();
    let adapter = Arc::new(PapegoyeAdapter::new(Arc::new(platform)));

    let start = {
        let adapter = Arc::clone(&adapter);
        let invocation = invocation("exec-1", shortcut_config());
        tokio::spawn(async move { adapter.execute(&invocation).await })
    };
    // Let exec-1 park inside its gated key-down, holding the operation lock.
    tokio::task::yield_now().await;

    let release = {
        let adapter = Arc::clone(&adapter);
        tokio::spawn(async move { adapter.release("exec-1").await })
    };
    tokio::task::yield_now().await;
    assert!(
        !release.is_finished(),
        "release must wait for the blocked start to settle"
    );

    release_tx.send(()).ok();
    let (start_result, release_result) = tokio::join!(start, release);
    assert_eq!(
        start_result.expect("start ran").status,
        ActionStatus::Started
    );
    assert!(release_result.expect("release ran").is_ok());

    let events = events.lock().expect("events lock");
    assert_eq!(events.downs, vec![0x3F, 0x31], "hold posted");
    assert_eq!(
        events.ups,
        vec![0x31, 0x3F],
        "release posted after the start settled, key first"
    );
    assert_eq!(events.downs.len(), events.ups.len(), "no stuck keys");
}

#[tokio::test]
async fn shutdown_waits_for_a_blocked_start_and_balances() {
    // Same invariant for shutdown: `release_all` must not drain the tracking
    // state while a start is still injecting. It waits, then releases the
    // completed hold, leaving nothing to release afterwards.
    let (release_tx, release_rx) = oneshot::channel();
    let platform = MockPlatform::new().with_hold_down(release_rx);
    let events = platform.events.clone();
    let adapter = Arc::new(PapegoyeAdapter::new(Arc::new(platform)));

    let start = {
        let adapter = Arc::clone(&adapter);
        let invocation = invocation("exec-1", shortcut_config());
        tokio::spawn(async move { adapter.execute(&invocation).await })
    };
    tokio::task::yield_now().await;

    let shutdown = {
        let adapter = Arc::clone(&adapter);
        tokio::spawn(async move { adapter.release_all().await })
    };
    tokio::task::yield_now().await;
    assert!(
        !shutdown.is_finished(),
        "shutdown must wait for the blocked start to settle"
    );

    release_tx.send(()).ok();
    let (start_result, released) = tokio::join!(start, shutdown);
    assert_eq!(
        start_result.expect("start ran").status,
        ActionStatus::Started
    );
    let mut released = released.expect("shutdown ran");
    released.sort_unstable();
    assert_eq!(released, vec![0x31, 0x3F], "shutdown released the hold");

    {
        let events = events.lock().expect("events lock");
        assert_eq!(events.downs.len(), events.ups.len(), "no stuck keys");
    }

    // The tracking map is clear after shutdown: nothing is left to release.
    assert!(matches!(
        adapter.release("exec-1").await,
        Err(AdapterError::UnknownExecution(_))
    ));
}

#[tokio::test]
async fn validate_requires_exactly_one_of_shortcut_or_keycode() {
    let (adapter, _) = adapter();

    let neither = adapter.validate(&json!({})).await;
    assert!(!neither.valid);
    assert!(neither
        .errors
        .iter()
        .any(|error| error.contains("must set")));

    let both = adapter
        .validate(&json!({ "shortcut": "F17", "keycode": 64 }))
        .await;
    assert!(!both.valid);
    assert!(both
        .errors
        .iter()
        .any(|error| error.contains("exactly one of")));

    assert!(adapter.validate(&shortcut_config()).await.valid);
    assert!(adapter.validate(&json!({ "keycode": 64 })).await.valid);
}

#[tokio::test]
async fn validate_rejects_unknown_shortcuts_and_modifiers() {
    let (adapter, _) = adapter();

    let unknown_shortcut = adapter.validate(&json!({ "shortcut": "not-a-key" })).await;
    assert!(!unknown_shortcut.valid);
    assert!(unknown_shortcut
        .errors
        .iter()
        .any(|error| error.contains("does not resolve")));

    let unknown_modifier = adapter
        .validate(&json!({ "keycode": 64, "modifiers": ["nope"] }))
        .await;
    assert!(!unknown_modifier.valid);
    assert!(unknown_modifier
        .errors
        .iter()
        .any(|error| error.contains("unknown modifier")));
}

#[tokio::test]
async fn execute_fails_explicitly_without_config() {
    let (adapter, events) = adapter();
    let result = adapter.execute(&invocation("exec-1", json!({}))).await;
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result
        .message
        .as_deref()
        .is_some_and(|message| message.contains("must set")));

    let events = events.lock().expect("events lock");
    assert!(events.downs.is_empty());
}

#[tokio::test]
async fn execute_rejects_unsupported_actions() {
    let (adapter, events) = adapter();
    let mut invocation = invocation("exec-1", shortcut_config());
    invocation.action_id = "git.commit".into();
    let result = adapter.execute(&invocation).await;
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result
        .message
        .as_deref()
        .is_some_and(|message| message.contains("unsupported action")));

    let events = events.lock().expect("events lock");
    assert!(events.downs.is_empty());
}
