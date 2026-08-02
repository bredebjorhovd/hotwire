//! Mocked integration tests for the Herdr adapter.
//!
//! The platform is substituted with a scriptable mock so detection, capability
//! negotiation, fallback ordering, and validation are exercised without any
//! real OS side effects.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use hotwire_adapter_herdr::{
    HerdrAdapter, HerdrCapability, HerdrConfig, HerdrError, HerdrPlatform,
};
use hotwire_adapter_sdk::{ActionInvocation, Adapter, ExecutionContext, KeyCombo};
use hotwire_core::{ActionStatus, Trigger};
use serde_json::{json, Value};

/// Calls recorded by the mock platform.
#[derive(Default)]
struct MockCalls {
    probes: Vec<String>,
    api_calls: Vec<(String, String)>,
    deep_links: Vec<String>,
    launches: Vec<(Option<String>, Option<String>)>,
    shortcuts: Vec<String>,
}

/// A scriptable Herdr platform for tests.
struct MockPlatform {
    api_version: Option<String>,
    app_present: bool,
    api_call_result: Result<(), HerdrError>,
    deep_link_result: Result<(), HerdrError>,
    launch_result: Result<(), HerdrError>,
    shortcut_result: Result<(), HerdrError>,
    calls: Arc<Mutex<MockCalls>>,
}

impl MockPlatform {
    fn new() -> Self {
        Self {
            api_version: None,
            app_present: false,
            api_call_result: Ok(()),
            deep_link_result: Ok(()),
            launch_result: Ok(()),
            shortcut_result: Ok(()),
            calls: Arc::new(Mutex::new(MockCalls::default())),
        }
    }
}

#[async_trait]
impl HerdrPlatform for MockPlatform {
    async fn probe_local_api(&self, base_url: &str) -> Option<String> {
        self.calls
            .lock()
            .expect("calls lock")
            .probes
            .push(base_url.to_string());
        self.api_version.clone()
    }

    async fn call_local_api(&self, base_url: &str, action: &str) -> Result<(), HerdrError> {
        self.calls
            .lock()
            .expect("calls lock")
            .api_calls
            .push((base_url.to_string(), action.to_string()));
        self.api_call_result.clone()
    }

    async fn open_deep_link(&self, url: &str) -> Result<(), HerdrError> {
        self.calls
            .lock()
            .expect("calls lock")
            .deep_links
            .push(url.to_string());
        self.deep_link_result.clone()
    }

    fn app_available(&self, bundle_id: Option<&str>, _app_path: Option<&str>) -> bool {
        self.app_present && bundle_id.is_some_and(|bundle| !bundle.is_empty())
    }

    async fn launch_or_focus(
        &self,
        bundle_id: Option<&str>,
        app_path: Option<&str>,
    ) -> Result<(), HerdrError> {
        self.calls.lock().expect("calls lock").launches.push((
            bundle_id.map(ToString::to_string),
            app_path.map(ToString::to_string),
        ));
        self.launch_result.clone()
    }

    fn resolve_shortcut(&self, shortcut: &str) -> Option<KeyCombo> {
        match shortcut {
            "F17" => Some(KeyCombo {
                modifiers: Vec::new(),
                key: 0x40,
            }),
            "fn+space" => Some(KeyCombo {
                modifiers: vec![0x3F],
                key: 0x31,
            }),
            _ => None,
        }
    }

    async fn send_shortcut(&self, shortcut: &str) -> Result<(), HerdrError> {
        self.calls
            .lock()
            .expect("calls lock")
            .shortcuts
            .push(shortcut.to_string());
        self.shortcut_result.clone()
    }
}

fn invocation(config: Value) -> ActionInvocation {
    ActionInvocation {
        execution_id: "exec-1".into(),
        action_id: "app.open_or_focus".into(),
        adapter_id: "herdr".into(),
        profile_id: "p".into(),
        binding_id: "b".into(),
        trigger: Trigger::Press,
        config,
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

fn build(platform: MockPlatform) -> (HerdrAdapter, Arc<Mutex<MockCalls>>) {
    let calls = platform.calls.clone();
    (HerdrAdapter::new(Arc::new(platform)), calls)
}

#[tokio::test]
async fn detection_prefers_api_then_app_then_absent() {
    let (with_api, _) = build(MockPlatform::new().with_api("2.1.0"));
    let result = with_api.detect().await;
    assert!(result.detected);
    assert_eq!(result.version.as_deref(), Some("2.1.0"));

    let (app_present, _) = build(MockPlatform::new().with_app_present());
    let result = app_present.detect().await;
    assert!(result.detected);
    assert_eq!(result.version, None);

    let (absent, _) = build(MockPlatform::new());
    let result = absent.detect().await;
    assert!(!result.detected);
}

#[tokio::test]
async fn negotiate_follows_the_spec_preference_order() {
    let (adapter, _) = build(MockPlatform::new().with_api("1.0.0").with_app_present());
    let config = HerdrConfig {
        api_base_url: Some("http://127.0.0.1:7398".into()),
        bundle_id: Some("dev.herdr.app".into()),
        ..HerdrConfig::default()
    };
    // Both API and app available: API wins.
    assert!(matches!(
        adapter.negotiate(&config).await,
        Some(HerdrCapability::LocalApi { version, .. }) if version == "1.0.0"
    ));

    let (app_only, _) = build(MockPlatform::new().with_app_present());
    let config = HerdrConfig {
        bundle_id: Some("dev.herdr.app".into()),
        ..HerdrConfig::default()
    };
    assert!(matches!(
        app_only.negotiate(&config).await,
        Some(HerdrCapability::App { bundle_id, .. }) if bundle_id == "dev.herdr.app"
    ));

    let (shortcut_only, _) = build(MockPlatform::new());
    let config = HerdrConfig {
        shortcut: Some("F17".into()),
        ..HerdrConfig::default()
    };
    assert!(matches!(
        shortcut_only.negotiate(&config).await,
        Some(HerdrCapability::Shortcut { shortcut }) if shortcut == "F17"
    ));

    // Nothing configured or available: negotiation is explicit about absence.
    assert!(adapter.negotiate(&HerdrConfig::default()).await.is_none());
}

#[tokio::test]
async fn validate_requires_at_least_one_integration_path() {
    let (adapter, _) = build(MockPlatform::new());
    let result = adapter.validate(&json!({})).await;
    assert!(!result.valid);
    assert!(result
        .errors
        .iter()
        .any(|error| error.contains("at least one integration path")));
}

#[tokio::test]
async fn validate_rejects_malformed_paths_and_unknown_shortcuts() {
    let (adapter, _) = build(MockPlatform::new());

    let result = adapter
        .validate(
            &json!({ "apiBaseUrl": "herdr://focus", "deepLink": "focus", "shortcut": "not-a-key" }),
        )
        .await;
    assert!(!result.valid);
    assert!(result
        .errors
        .iter()
        .any(|error| error.contains("apiBaseUrl")));
    assert!(result
        .errors
        .iter()
        .any(|error| error.contains("deepLink must include a scheme")));
    assert!(result
        .errors
        .iter()
        .any(|error| error.contains("shortcut `not-a-key` does not resolve")));
}

#[tokio::test]
async fn validate_rejects_remote_api_hosts() {
    let (adapter, _) = build(MockPlatform::new());

    for config in [
        json!({ "apiBaseUrl": "http://example.com:7398" }),
        json!({ "apiBaseUrl": "http://192.168.1.10:7398" }),
    ] {
        let result = adapter.validate(&config).await;
        assert!(!result.valid, "config {config} must not validate");
        assert!(
            result.errors.iter().any(|error| error.contains("loopback")),
            "config {config}"
        );
    }
}

#[tokio::test]
async fn validate_accepts_each_integration_path() {
    let (adapter, _) = build(MockPlatform::new());

    for config in [
        json!({ "apiBaseUrl": "http://127.0.0.1:7398" }),
        json!({ "deepLink": "herdr://actions/focus" }),
        json!({ "bundleId": "dev.herdr.app" }),
        json!({ "appPath": "/Applications/Herdr.app" }),
        json!({ "shortcut": "fn+space" }),
    ] {
        assert!(
            adapter.validate(&config).await.valid,
            "config {config} should validate"
        );
    }
}

#[tokio::test]
async fn execute_uses_the_local_api_when_detected() {
    let platform = MockPlatform::new().with_api("1.0.0");
    let (adapter, calls) = build(platform);

    let result = adapter
        .execute(&invocation(
            json!({ "apiBaseUrl": "http://127.0.0.1:7398" }),
        ))
        .await;
    assert_eq!(result.status, ActionStatus::Succeeded);
    assert!(result
        .message
        .as_deref()
        .is_some_and(|m| m.contains("local API")));

    let calls = calls.lock().expect("calls lock");
    assert_eq!(
        calls.api_calls,
        vec![("http://127.0.0.1:7398".into(), "focus".into())]
    );
    assert!(calls.launches.is_empty());
    assert!(calls.shortcuts.is_empty());
}

#[tokio::test]
async fn execute_opens_a_deep_link_when_the_api_is_unreachable() {
    let platform = MockPlatform::new(); // no API, no app
    let (adapter, calls) = build(platform);

    let result = adapter
        .execute(&invocation(json!({ "deepLink": "herdr://actions/focus" })))
        .await;
    assert_eq!(result.status, ActionStatus::Succeeded);
    assert!(result
        .message
        .as_deref()
        .is_some_and(|m| m.contains("deep link")));

    let calls = calls.lock().expect("calls lock");
    assert_eq!(calls.deep_links, vec!["herdr://actions/focus"]);
}

#[tokio::test]
async fn execute_falls_back_to_app_when_the_api_call_fails() {
    let platform = MockPlatform::new()
        .with_api("1.0.0")
        .with_api_call(Err(HerdrError::Api("500".into())))
        .with_app_present();
    let (adapter, calls) = build(platform);

    let result = adapter
        .execute(&invocation(json!({
            "apiBaseUrl": "http://127.0.0.1:7398",
            "bundleId": "dev.herdr.app"
        })))
        .await;
    assert_eq!(result.status, ActionStatus::Succeeded);
    assert!(result.message.as_deref().is_some_and(|m| m.contains("app")));

    let calls = calls.lock().expect("calls lock");
    assert_eq!(calls.api_calls.len(), 1);
    assert_eq!(calls.launches.len(), 1);
}

#[tokio::test]
async fn execute_falls_back_to_the_shortcut_when_no_app_exists() {
    let platform = MockPlatform::new(); // no API, no app
    let (adapter, calls) = build(platform);

    let result = adapter
        .execute(&invocation(json!({
            "bundleId": "dev.herdr.app",
            "shortcut": "F17"
        })))
        .await;
    assert_eq!(result.status, ActionStatus::Succeeded);
    assert!(result
        .message
        .as_deref()
        .is_some_and(|m| m.contains("shortcut")));

    let calls = calls.lock().expect("calls lock");
    assert_eq!(calls.shortcuts, vec!["F17"]);
    assert!(
        calls.launches.is_empty(),
        "no app present, so no launch attempted"
    );
}

#[tokio::test]
async fn execute_fails_when_every_tier_fails() {
    let platform = MockPlatform::new()
        .with_api("1.0.0")
        .with_api_call(Err(HerdrError::Api("500".into())))
        .with_app_present()
        .with_launch(Err(HerdrError::Launch("crashed".into())))
        .with_shortcut(Err(HerdrError::Shortcut("injection denied".into())));
    let (adapter, _) = build(platform);

    let result = adapter
        .execute(&invocation(json!({
            "apiBaseUrl": "http://127.0.0.1:7398",
            "bundleId": "dev.herdr.app",
            "shortcut": "F17"
        })))
        .await;
    assert_eq!(result.status, ActionStatus::Failed);
    let message = result.message.expect("failure message");
    assert!(message.contains("local API"));
    assert!(message.contains("app launch"));
    assert!(message.contains("shortcut"));
}

#[tokio::test]
async fn execute_fails_explicitly_without_any_configured_integration() {
    let (adapter, _) = build(MockPlatform::new());
    let result = adapter.execute(&invocation(json!({}))).await;
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result
        .message
        .as_deref()
        .is_some_and(|m| m.contains("no Herdr integration is configured")));
}

#[tokio::test]
async fn execute_rejects_unsupported_actions() {
    let platform = MockPlatform::new().with_api("1.0.0");
    let (adapter, _) = build(platform);

    let mut invocation = invocation(json!({ "apiBaseUrl": "http://127.0.0.1:7398" }));
    invocation.action_id = "git.commit".into();
    let result = adapter.execute(&invocation).await;
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result
        .message
        .as_deref()
        .is_some_and(|m| m.contains("unsupported action")));
}

#[tokio::test]
async fn cancel_reports_unknown_executions() {
    let (adapter, _) = build(MockPlatform::new());
    assert!(adapter.cancel("nope").await.is_err());
}

// --- builder helpers for the mock platform ---

impl MockPlatform {
    fn with_api(mut self, version: &str) -> Self {
        self.api_version = Some(version.to_string());
        self
    }

    fn with_app_present(mut self) -> Self {
        self.app_present = true;
        self
    }

    fn with_api_call(mut self, result: Result<(), HerdrError>) -> Self {
        self.api_call_result = result;
        self
    }

    fn with_launch(mut self, result: Result<(), HerdrError>) -> Self {
        self.launch_result = result;
        self
    }

    fn with_shortcut(mut self, result: Result<(), HerdrError>) -> Self {
        self.shortcut_result = result;
        self
    }
}
