//! Shell-side adapter registry and receipt plumbing.
//!
//! This is the vertical-slice bridge between the typed IPC commands and the
//! first-party adapters (ADP-001): it registers Herdr and Papegøye, runs
//! invocations, ends and cancels hold executions, and turns every outcome into
//! an [`ActionReceipt`] that the shell broadcasts to the UI. All OS-side work
//! happens inside the adapters behind their platform seams.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use hotwire_adapter_herdr::{default_platform as herdr_platform, HerdrAdapter};
use hotwire_adapter_papegoye::{default_platform as papegoye_platform, PapegoyeAdapter};
use hotwire_adapter_sdk::{
    ActionInvocation, ActionResult, DetectionResult, ExecutionContext, ValidationResult,
};
use hotwire_core::{ActionReceipt, ActionStatus, Trigger};
use hotwire_router::AdapterRegistry;
use serde_json::Value;

/// The adapters the shell owns, together with the state needed to end or
/// cancel in-flight executions from the UI.
pub struct AdapterState {
    registry: AdapterRegistry,
    active: Mutex<HashMap<String, ActionInvocation>>,
    next_execution: AtomicU64,
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
            .register(Arc::new(PapegoyeAdapter::new(papegoye_platform())))
            .expect("papegoye adapter registers exactly once");
        Self {
            registry,
            active: Mutex::new(HashMap::new()),
            next_execution: AtomicU64::new(0),
        }
    }

    /// Runs one action through its adapter and returns the resulting receipt.
    ///
    /// A `hold` execution that reports `Started` stays tracked so the UI can
    /// later end it with [`AdapterState::release`] or [`AdapterState::cancel`].
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
        self.finish_tracked(adapter_id, execution_id, physical_code, |_| {
            self.registry.release(adapter_id, execution_id)
        })
        .await
    }

    /// Cancels a tracked execution and returns its `Cancelled` receipt.
    pub async fn cancel(
        &self,
        adapter_id: &str,
        execution_id: &str,
        physical_code: &str,
    ) -> ActionReceipt {
        self.finish_tracked(adapter_id, execution_id, physical_code, |_| {
            self.registry.cancel(adapter_id, execution_id)
        })
        .await
    }

    /// Ends every tracked execution (used on shutdown).
    pub async fn release_active(&self) -> usize {
        let ids: Vec<(String, String, String)> = self
            .active
            .lock()
            .expect("active lock")
            .iter()
            .map(|(id, invocation)| {
                (
                    id.clone(),
                    invocation.adapter_id.clone(),
                    invocation.binding_id.clone(),
                )
            })
            .collect();
        for (execution_id, adapter_id, _) in &ids {
            let _ = self.registry.release(adapter_id, execution_id).await;
        }
        ids.len()
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
    async fn finish_tracked<F, Fut>(
        &self,
        adapter_id: &str,
        execution_id: &str,
        physical_code: &str,
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
            .remove(execution_id)
        else {
            return failed_receipt(
                adapter_id,
                execution_id,
                physical_code,
                format!("no active execution `{execution_id}` to end"),
            );
        };
        let result = match finish(&invocation).await {
            Ok(()) => ActionResult {
                execution_id: execution_id.to_string(),
                status: ActionStatus::Succeeded,
                message: None,
            },
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
    use serde_json::json;

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
}
