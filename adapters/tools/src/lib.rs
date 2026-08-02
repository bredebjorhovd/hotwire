//! Small, explicit adapters for the non-agent Numpad controls.

use async_trait::async_trait;
use hotwire_adapter_sdk::{
    ActionInvocation, ActionResult, Adapter, AdapterError, AdapterManifest, DetectionResult,
    ValidationResult,
};
use hotwire_core::ActionStatus;
use serde_json::{json, Value};
use std::path::PathBuf;

pub struct ToolAdapter {
    manifest: AdapterManifest,
}

impl ToolAdapter {
    #[must_use]
    pub fn new(id: &str, name: &str, capabilities: &[&str]) -> Self {
        Self {
            manifest: AdapterManifest {
                id: id.into(),
                name: name.into(),
                version: env!("CARGO_PKG_VERSION").into(),
                icon: id.into(),
                capabilities: capabilities.iter().map(|v| (*v).into()).collect(),
                config_schema: json!({"type":"object"}),
            },
        }
    }

    fn program(&self) -> Option<&'static str> {
        match self.manifest.id.as_str() {
            "claude-code" => Some("claude"),
            "codex" => Some("codex"),
            "terminal" => Some("open"),
            "git" => Some("git"),
            _ => None,
        }
    }
}

#[async_trait]
impl Adapter for ToolAdapter {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }
    async fn detect(&self) -> DetectionResult {
        let Some(program) = self.program() else {
            return DetectionResult {
                id: self.manifest.id.clone(),
                detected: false,
                version: None,
                path: None,
            };
        };
        let path = tokio::process::Command::new("which")
            .arg(program)
            .output()
            .await
            .ok()
            .filter(|o| o.status.success())
            .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()));
        DetectionResult {
            id: self.manifest.id.clone(),
            detected: path.is_some(),
            version: None,
            path,
        }
    }
    async fn validate(&self, _config: &Value) -> ValidationResult {
        ValidationResult {
            valid: true,
            errors: vec![],
        }
    }
    async fn execute(&self, invocation: &ActionInvocation) -> ActionResult {
        let result = match (self.manifest.id.as_str(), invocation.action_id.as_str()) {
            ("terminal", "terminal.open") => tokio::process::Command::new("open").args(["-a", "Terminal"]).output().await.map_err(|e| e.to_string()),
            ("claude-code", "claude.launch") => tokio::process::Command::new("open").args(["-a", "Claude"]).output().await.map_err(|e| e.to_string()),
            ("codex", "codex.launch") => tokio::process::Command::new("open").args(["-a", "Codex"]).output().await.map_err(|e| e.to_string()),
            ("git", "git.diff") => tokio::process::Command::new("git").args(["diff", "--stat"]).output().await.map_err(|e| e.to_string()),
            ("terminal", "test.run") => Err("test.run requires an approved project command; configure it in the runner review surface".into()),
            ("git", "git.commit" | "git.pr") => Err("mutating Git actions require explicit runner approval before execution".into()),
            _ => Err(format!("unsupported {} action `{}`", self.manifest.id, invocation.action_id)),
        };
        match result {
            Ok(output) if output.status.success() => ActionResult {
                execution_id: invocation.execution_id.clone(),
                status: ActionStatus::Succeeded,
                message: Some(format!("{} completed", invocation.action_id)),
            },
            Ok(output) => ActionResult {
                execution_id: invocation.execution_id.clone(),
                status: ActionStatus::Failed,
                message: Some(String::from_utf8_lossy(&output.stderr).trim().to_string()),
            },
            Err(message) => ActionResult {
                execution_id: invocation.execution_id.clone(),
                status: ActionStatus::Failed,
                message: Some(message),
            },
        }
    }
    async fn cancel(&self, execution_id: &str) -> Result<(), AdapterError> {
        Err(AdapterError::UnknownExecution(execution_id.into()))
    }
}
