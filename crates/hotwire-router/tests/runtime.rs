//! Runtime tests: dispatch, hold release, cancellation, and receipts.

mod common;

use std::sync::Arc;

use hotwire_core::{ActionReceipt, ActionStatus, KeyState, Trigger};
use hotwire_profile::Profile;
use hotwire_router::{HotwireRuntime, RouterConfig, RouterError, RuntimeError};

use crate::common::{binding, event, profile, TestAdapter};

#[tokio::test]
async fn disabled_profile_is_rejected_and_never_routes() {
    let disabled = Profile {
        enabled: false,
        ..profile(
            None,
            vec![binding("b", "Numpad5", Trigger::Press, "app.x", true)],
        )
    };

    let runtime = HotwireRuntime::new(disabled, RouterConfig::default());
    assert!(matches!(runtime, Err(RouterError::ProfileDisabled)));
}

#[tokio::test]
async fn press_dispatches_once_and_publishes_started_then_succeeded() {
    let adapter = TestAdapter::new(ActionStatus::Succeeded);
    let calls = adapter.calls.clone();
    let mut runtime = HotwireRuntime::new(
        profile(
            None,
            vec![binding("b", "Numpad5", Trigger::Press, "app.x", true)],
        ),
        RouterConfig::default(),
    )
    .expect("runtime should build");
    runtime
        .registry_mut()
        .register(Arc::new(adapter))
        .expect("adapter should register");

    let mut receipts = runtime.subscribe_receipts();

    let down = runtime.on_event(&event(0, "Numpad5", KeyState::Down)).await;
    assert!(down.consume_original);
    assert_eq!(down.invocations.len(), 1);
    assert_eq!(down.invocations[0].execution_id, "exec-1");

    let started = receipts.try_recv().expect("started receipt");
    assert_eq!(started.status, ActionStatus::Started);
    let succeeded = receipts.try_recv().expect("succeeded receipt");
    assert_eq!(succeeded.status, ActionStatus::Succeeded);
    assert!(receipts.try_recv().is_err(), "no further receipts expected");

    assert_eq!(
        calls.lock().expect("lock").executed,
        vec!["exec-1".to_string()]
    );
}

#[tokio::test]
async fn hold_starts_on_down_releases_on_up_without_repeats() {
    let adapter = TestAdapter::new(ActionStatus::Started);
    let calls = adapter.calls.clone();
    let mut runtime = HotwireRuntime::new(
        profile(
            None,
            vec![binding("h", "Numpad0", Trigger::Hold, "voice.input", true)],
        ),
        RouterConfig::default(),
    )
    .expect("runtime should build");
    runtime
        .registry_mut()
        .register(Arc::new(adapter))
        .expect("adapter should register");

    let mut receipts = runtime.subscribe_receipts();

    let down = runtime.on_event(&event(0, "Numpad0", KeyState::Down)).await;
    assert_eq!(down.invocations.len(), 1);
    let started = receipts.try_recv().expect("started receipt");
    assert_eq!(started.status, ActionStatus::Started);
    assert!(receipts.try_recv().is_err());

    let up = runtime.on_event(&event(50, "Numpad0", KeyState::Up)).await;
    assert_eq!(up.releases.len(), 1);
    assert_eq!(up.releases[0].execution_id, "exec-1");
    let succeeded = receipts.try_recv().expect("succeeded receipt");
    assert_eq!(succeeded.status, ActionStatus::Succeeded);

    assert_eq!(
        calls.lock().expect("lock").executed,
        vec!["exec-1".to_string()]
    );
    assert_eq!(
        calls.lock().expect("lock").released,
        vec!["exec-1".to_string()]
    );
}

#[tokio::test]
async fn cancel_tracks_and_cancels_active_executions() {
    let adapter = TestAdapter::new(ActionStatus::Started);
    let calls = adapter.calls.clone();
    let mut runtime = HotwireRuntime::new(
        profile(
            None,
            vec![binding("h", "Numpad0", Trigger::Hold, "voice.input", true)],
        ),
        RouterConfig::default(),
    )
    .expect("runtime should build");
    runtime
        .registry_mut()
        .register(Arc::new(adapter))
        .expect("adapter should register");

    let mut receipts = runtime.subscribe_receipts();

    runtime.on_event(&event(0, "Numpad0", KeyState::Down)).await;
    assert!(receipts.try_recv().is_ok());

    runtime
        .cancel("exec-1")
        .await
        .expect("cancel should succeed");
    let cancelled = receipts.try_recv().expect("cancelled receipt");
    assert_eq!(cancelled.status, ActionStatus::Cancelled);
    assert_eq!(
        calls.lock().expect("lock").cancelled,
        vec!["exec-1".to_string()]
    );

    assert!(matches!(
        runtime.cancel("exec-1").await,
        Err(RuntimeError::UnknownExecution(_))
    ));
}

#[tokio::test]
async fn cancel_active_ends_every_in_flight_hold() {
    let adapter = TestAdapter::new(ActionStatus::Started);
    let calls = adapter.calls.clone();
    let mut runtime = HotwireRuntime::new(
        profile(
            None,
            vec![
                binding("h1", "Numpad0", Trigger::Hold, "voice.input", true),
                binding("h2", "Numpad1", Trigger::Hold, "voice.input", true),
            ],
        ),
        RouterConfig::default(),
    )
    .expect("runtime should build");
    runtime
        .registry_mut()
        .register(Arc::new(adapter))
        .expect("adapter should register");

    let mut receipts = runtime.subscribe_receipts();

    runtime.on_event(&event(0, "Numpad0", KeyState::Down)).await;
    runtime.on_event(&event(0, "Numpad1", KeyState::Down)).await;
    assert_eq!(calls.lock().expect("lock").executed.len(), 2);

    let cancelled = runtime.cancel_active().await;
    assert_eq!(cancelled, 2);
    assert_eq!(calls.lock().expect("lock").cancelled.len(), 2);

    let mut saw_cancelled = 0;
    while let Ok(receipt) = receipts.try_recv() {
        if receipt.status == ActionStatus::Cancelled {
            saw_cancelled += 1;
        }
    }
    assert_eq!(saw_cancelled, 2);
}

#[tokio::test]
async fn unregistered_adapter_fails_gracefully() {
    let mut runtime = HotwireRuntime::new(
        profile(
            None,
            vec![{
                let mut b = binding("b", "Numpad5", Trigger::Press, "app.x", true);
                b.adapter_id = "missing".to_string();
                b
            }],
        ),
        RouterConfig::default(),
    )
    .expect("runtime should build");

    let mut receipts = runtime.subscribe_receipts();

    runtime.on_event(&event(0, "Numpad5", KeyState::Down)).await;

    let started = receipts.try_recv().expect("started receipt");
    assert_eq!(started.status, ActionStatus::Started);
    let failed = receipts.try_recv().expect("failed receipt");
    assert_eq!(failed.status, ActionStatus::Failed);
    assert!(failed
        .message
        .as_deref()
        .is_some_and(|message| message.contains("not registered")));
}

#[tokio::test]
async fn press_receipt_carries_route_context() {
    let adapter = TestAdapter::new(ActionStatus::Succeeded);
    let mut runtime = HotwireRuntime::new(
        profile(
            None,
            vec![binding("b", "Numpad5", Trigger::Press, "app.x", true)],
        ),
        RouterConfig::default(),
    )
    .expect("runtime should build");
    runtime
        .registry_mut()
        .register(Arc::new(adapter))
        .expect("adapter should register");

    let mut receipts = runtime.subscribe_receipts();
    runtime.on_event(&event(0, "Numpad5", KeyState::Down)).await;

    let started: ActionReceipt = receipts.try_recv().expect("started receipt");
    assert_eq!(started.physical_code, "Numpad5");
    assert_eq!(started.action_id, "app.x");
    assert_eq!(started.adapter_id, "test");
    assert_eq!(started.profile_id, "p");
    assert_eq!(started.binding_id, "b");
}
