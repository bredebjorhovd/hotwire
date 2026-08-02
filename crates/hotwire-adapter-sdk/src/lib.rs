//! Adapter execution boundary.
//!
//! Adapters turn semantic actions into concrete executions (launch an app,
//! send a shortcut, open a URL, talk to a CLI tool). This crate defines the
//! contracts adapters implement and the types that cross the execution
//! boundary. It deliberately contains no adapters themselves; the first-party
//! adapters land in `adapters/` with ADP-001.

use std::path::PathBuf;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use hotwire_core::{ActionStatus, Trigger};

/// Errors an adapter may return for lifecycle operations.
#[derive(Debug, Error)]
pub enum AdapterError {
    /// The adapter was asked to cancel an execution it does not own.
    #[error("adapter cannot cancel unknown execution `{0}`")]
    UnknownExecution(String),
    /// Any other adapter failure.
    #[error("adapter operation failed: {0}")]
    Other(String),
}

/// Static identity and capabilities of an adapter.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub icon: String,
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub config_schema: Value,
}

/// The foreground application, when the OS can identify one.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveApplication {
    pub bundle_id: Option<String>,
    pub process_name: String,
}

/// Everything an adapter needs to know about the moment an action fired.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionContext {
    pub active_application: Option<ActiveApplication>,
    pub cwd: Option<String>,
    pub profile_id: String,
    pub binding_id: String,
    pub trigger: Trigger,
    pub timestamp: String,
}

/// A concrete request to run one semantic action through an adapter.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionInvocation {
    pub execution_id: String,
    pub action_id: String,
    pub adapter_id: String,
    pub profile_id: String,
    pub binding_id: String,
    pub trigger: Trigger,
    pub config: Value,
    pub context: ExecutionContext,
}

/// The outcome of an adapter execution, ready for the live board and logs.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionResult {
    pub execution_id: String,
    pub status: ActionStatus,
    pub message: Option<String>,
}

/// Result of probing whether an integration is available on this machine.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectionResult {
    pub id: String,
    pub detected: bool,
    pub version: Option<String>,
    pub path: Option<PathBuf>,
}

/// Result of validating a binding's adapter configuration.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
}

/// Contract every Hotwire adapter implements.
#[async_trait]
pub trait Adapter: Send + Sync {
    /// Static identity and capability declaration.
    fn manifest(&self) -> &AdapterManifest;

    /// Probe whether the underlying integration is available.
    async fn detect(&self) -> DetectionResult;

    /// Validate a binding configuration against the adapter's config schema.
    async fn validate(&self, config: &Value) -> ValidationResult;

    /// Execute an invocation and report the outcome.
    async fn execute(&self, invocation: &ActionInvocation) -> ActionResult;

    /// Best-effort cancellation of an in-flight execution.
    ///
    /// # Errors
    ///
    /// Returns [`AdapterError::UnknownExecution`] when `execution_id` does not
    /// belong to this adapter.
    async fn cancel(&self, execution_id: &str) -> Result<(), AdapterError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct EchoAdapter {
        manifest: AdapterManifest,
    }

    #[async_trait]
    impl Adapter for EchoAdapter {
        fn manifest(&self) -> &AdapterManifest {
            &self.manifest
        }

        async fn detect(&self) -> DetectionResult {
            DetectionResult {
                id: self.manifest.id.clone(),
                detected: true,
                version: Some("1.0.0".into()),
                path: None,
            }
        }

        async fn validate(&self, config: &Value) -> ValidationResult {
            ValidationResult {
                valid: config.get("command").is_some(),
                errors: if config.get("command").is_some() {
                    Vec::new()
                } else {
                    vec!["missing `command`".into()]
                },
            }
        }

        async fn execute(&self, invocation: &ActionInvocation) -> ActionResult {
            ActionResult {
                execution_id: invocation.execution_id.clone(),
                status: ActionStatus::Succeeded,
                message: Some("echoed".into()),
            }
        }

        async fn cancel(&self, execution_id: &str) -> Result<(), AdapterError> {
            Err(AdapterError::UnknownExecution(execution_id.to_string()))
        }
    }

    fn invocation(execution_id: &str) -> ActionInvocation {
        ActionInvocation {
            execution_id: execution_id.into(),
            action_id: "shell.run".into(),
            adapter_id: "echo".into(),
            profile_id: "p1".into(),
            binding_id: "b1".into(),
            trigger: Trigger::Press,
            config: json!({ "command": "true" }),
            context: ExecutionContext {
                active_application: None,
                cwd: None,
                profile_id: "p1".into(),
                binding_id: "b1".into(),
                trigger: Trigger::Press,
                timestamp: "2026-08-02T00:00:00Z".into(),
            },
        }
    }

    #[tokio::test]
    async fn object_safe_adapter_executes_typed_invocation() {
        let adapter: Box<dyn Adapter> = Box::new(EchoAdapter {
            manifest: AdapterManifest {
                id: "echo".into(),
                name: "Echo".into(),
                version: "0.1.0".into(),
                icon: "echo".into(),
                capabilities: vec!["shell".into()],
                config_schema: json!({}),
            },
        });

        assert_eq!(adapter.manifest().id, "echo");
        assert!(adapter.detect().await.detected);
        assert!(adapter.validate(&json!({ "command": "true" })).await.valid);
        assert!(!adapter.validate(&json!({})).await.errors.is_empty());

        let result = adapter.execute(&invocation("exec-1")).await;
        assert_eq!(result.status, ActionStatus::Succeeded);
    }

    #[tokio::test]
    async fn cancel_reports_unknown_executions() {
        let adapter = EchoAdapter {
            manifest: AdapterManifest {
                id: "echo".into(),
                name: "Echo".into(),
                version: "0.1.0".into(),
                icon: "echo".into(),
                capabilities: Vec::new(),
                config_schema: json!({}),
            },
        };

        assert!(matches!(
            adapter.cancel("nope").await,
            Err(AdapterError::UnknownExecution(_))
        ));
    }
}
