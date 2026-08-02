//! Shared helpers for the hotwire-router integration tests.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use hotwire_adapter_sdk::{
    ActionInvocation, ActionResult, Adapter, AdapterError, AdapterManifest, DetectionResult,
    ValidationResult,
};
use hotwire_core::{ActionStatus, KeyState, ModifierState, PhysicalKeyEvent, Trigger};
use hotwire_profile::{Binding, CaptureMode, ControlSurface, Profile};
use serde_json::Value;

/// Default double-press window, matching `RouterConfig::default`.
pub const DOUBLE_PRESS_NS: u64 = 250_000_000;

/// Builds a normalized key event.
pub fn event(timestamp_ns: u64, code: &str, state: KeyState) -> PhysicalKeyEvent {
    PhysicalKeyEvent {
        device_hint: None,
        scan_code: 0,
        physical_code: code.to_string(),
        state,
        modifiers: ModifierState::default(),
        timestamp_ns,
        is_repeat: false,
        is_injected: false,
    }
}

/// Builds a binding on the `test` adapter.
pub fn binding(id: &str, code: &str, trigger: Trigger, action_id: &str, consume: bool) -> Binding {
    Binding {
        id: id.to_string(),
        physical_code: code.to_string(),
        trigger,
        action_id: action_id.to_string(),
        adapter_id: "test".to_string(),
        config: serde_json::json!({}),
        consume_original: consume,
        enabled: true,
        layer: false,
    }
}

/// Builds an active profile for a set of bindings.
pub fn profile(layer_key: Option<&str>, bindings: Vec<Binding>) -> Profile {
    profile_with(CaptureMode::Capture, layer_key, bindings)
}

/// Builds an active profile with an explicit capture mode.
pub fn profile_with(
    capture_mode: CaptureMode,
    layer_key: Option<&str>,
    bindings: Vec<Binding>,
) -> Profile {
    Profile {
        schema_version: 1,
        id: "p".to_string(),
        name: "P".to_string(),
        control_surface: ControlSurface::Numpad,
        bindings,
        layer_key: layer_key.map(ToString::to_string),
        capture_mode,
        enabled: true,
    }
}

/// Calls recorded by a [`TestAdapter`].
#[derive(Default)]
pub struct Calls {
    pub executed: Vec<String>,
    pub cancelled: Vec<String>,
    pub released: Vec<String>,
}

/// A scriptable adapter that records every lifecycle call.
pub struct TestAdapter {
    manifest: AdapterManifest,
    pub calls: Arc<Mutex<Calls>>,
    execute_status: ActionStatus,
}

impl TestAdapter {
    /// Creates an adapter whose `execute` always returns `status`.
    pub fn new(status: ActionStatus) -> Self {
        Self {
            manifest: AdapterManifest {
                id: "test".to_string(),
                name: "Test".to_string(),
                version: "0.1.0".to_string(),
                icon: "test".to_string(),
                capabilities: Vec::new(),
                config_schema: serde_json::json!({}),
            },
            calls: Arc::new(Mutex::new(Calls::default())),
            execute_status: status,
        }
    }
}

#[async_trait]
impl Adapter for TestAdapter {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }

    async fn detect(&self) -> DetectionResult {
        DetectionResult {
            id: self.manifest.id.clone(),
            detected: true,
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
        self.calls
            .lock()
            .expect("calls lock")
            .executed
            .push(invocation.execution_id.clone());
        ActionResult {
            execution_id: invocation.execution_id.clone(),
            status: self.execute_status.clone(),
            message: None,
        }
    }

    async fn cancel(&self, execution_id: &str) -> Result<(), AdapterError> {
        self.calls
            .lock()
            .expect("calls lock")
            .cancelled
            .push(execution_id.to_string());
        Ok(())
    }

    async fn release(&self, execution_id: &str) -> Result<(), AdapterError> {
        self.calls
            .lock()
            .expect("calls lock")
            .released
            .push(execution_id.to_string());
        Ok(())
    }
}

/// Convenience import for the default window in tests.
pub fn double_press_window() -> Duration {
    Duration::from_nanos(DOUBLE_PRESS_NS)
}
