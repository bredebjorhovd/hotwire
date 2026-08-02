//! Mocked integration tests for the Papegøye adapter.
//!
//! The platform is substituted with a recorder so the push-to-talk hold state
//! machine — start, release, cancel, shutdown — is exercised without touching a
//! real keyboard. These tests pin the spec invariants (spec §13.5): a down is
//! always paired with exactly one up, never repeated, and never left stuck.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use hotwire_adapter_papegoye::{PapegoyeAdapter, PapegoyeError, PapegoyePlatform};
use hotwire_adapter_sdk::{ActionInvocation, Adapter, AdapterError, ExecutionContext};
use hotwire_core::{ActionStatus, Trigger};
use serde_json::{json, Value};

/// Synthetic key events recorded by the mock platform.
#[derive(Default, Debug)]
struct MockEvents {
    downs: Vec<u16>,
    ups: Vec<u16>,
    held: Vec<u16>,
}

/// A scriptable Papegøye platform that records every injected key event.
struct MockPlatform {
    app_present: bool,
    events: Arc<Mutex<MockEvents>>,
}

impl MockPlatform {
    fn new() -> Self {
        Self {
            app_present: true,
            events: Arc::new(Mutex::new(MockEvents::default())),
        }
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
    assert_eq!(events.ups, vec![0x3F, 0x31], "each keycode up once");
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
            vec![0x3F, 0x31],
            "release is attempted on cancel"
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
        assert_eq!(events.ups, vec![0x3F, 0x31], "exactly one up per keycode");
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

    let released = adapter.release_all().await;
    assert_eq!(released, vec![0x3F, 0x31], "each held key released once");

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

    let absent = PapegoyeAdapter::new(Arc::new(MockPlatform {
        app_present: false,
        events: Arc::new(Mutex::new(MockEvents::default())),
    }));
    assert!(!absent.detect().await.detected);
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
