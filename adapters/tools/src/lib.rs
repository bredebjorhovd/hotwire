//! Small, explicit adapters for the non-agent Numpad controls.

use async_trait::async_trait;
use hotwire_adapter_sdk::{
    ActionInvocation, ActionResult, Adapter, AdapterError, AdapterManifest, DetectionResult,
    ValidationResult,
};
use hotwire_core::ActionStatus;
use hotwire_runner::{CancellationToken, CommandRunner, CommandSpec, CwdStrategy, RunStatus};
use serde_json::{json, Value};
use std::path::PathBuf;

pub struct ToolAdapter {
    manifest: AdapterManifest,
    runner: CommandRunner,
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
            runner: CommandRunner::new(),
        }
    }

    async fn run_safe(&self, argv: Vec<String>, imported: bool) -> Result<String, String> {
        let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
        let spec = CommandSpec::new(argv)
            .with_cwd(CwdStrategy::Fixed(cwd))
            .with_open_terminal(false)
            .with_imported(imported);
        let output = self
            .runner
            .run(&spec, &CancellationToken::new(), None)
            .await;
        match output.status {
            RunStatus::Succeeded { .. } => Ok(output.stdout),
            RunStatus::ApprovalRequired(review) => Err(format!("approval required: {review}")),
            RunStatus::Failed { .. } | RunStatus::StartError(_) => Err(output.stderr),
            status => Err(format!("command ended with {status:?}")),
        }
    }

    fn program(&self) -> Option<&'static str> {
        match self.manifest.id.as_str() {
            "claude-code" => Some("claude"),
            "codex" => Some("codex"),
            "terminal" | "app" | "shortcut" => Some("open"),
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
            ("terminal", "terminal.open") => tokio::process::Command::new("open")
                .args(["-a", "Terminal"])
                .output()
                .await
                .map_err(|e| e.to_string()),
            ("claude-code", "claude.launch") => tokio::process::Command::new("open")
                .args(["-a", "Claude"])
                .output()
                .await
                .map_err(|e| e.to_string()),
            ("codex", "codex.launch") => tokio::process::Command::new("open")
                .args(["-a", "Codex"])
                .output()
                .await
                .map_err(|e| e.to_string()),
            ("git", "git.diff") => self
                .run_safe(vec!["git".into(), "diff".into(), "--stat".into()], false)
                .await
                .map(|stdout| std::process::Output {
                    status: std::process::ExitStatus::default(),
                    stdout: stdout.into_bytes(),
                    stderr: vec![],
                }),
            ("terminal", "test.run") => self
                .run_safe(vec!["pnpm".into(), "test".into()], false)
                .await
                .map(|stdout| std::process::Output {
                    status: std::process::ExitStatus::default(),
                    stdout: stdout.into_bytes(),
                    stderr: vec![],
                }),
            ("git", "git.commit" | "git.pr") => self
                .run_safe(
                    vec![
                        "git".into(),
                        invocation
                            .action_id
                            .strip_prefix("git.")
                            .unwrap_or("status")
                            .into(),
                    ],
                    true,
                )
                .await
                .map(|stdout| std::process::Output {
                    status: std::process::ExitStatus::default(),
                    stdout: stdout.into_bytes(),
                    stderr: vec![],
                }),
            ("app", "app.open_or_focus") => {
                let app = invocation
                    .config
                    .get("appName")
                    .and_then(Value::as_str)
                    .unwrap_or("Terminal");
                let bundle = invocation.config.get("bundleId").and_then(Value::as_str);
                let mut command = tokio::process::Command::new("open");
                if let Some(bundle) = bundle {
                    command.args(["-b", bundle]);
                } else {
                    command.args(["-a", app]);
                }
                command.output().await.map_err(|e| e.to_string())
            }
            ("shortcut", "profile.switch" | "shortcut.send") => {
                let shortcut = invocation
                    .config
                    .get("shortcut")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "shortcut config must include `shortcut`".to_string());
                shortcut.and_then(send_shortcut)
            }
            _ => Err(format!(
                "unsupported {} action `{}`",
                self.manifest.id, invocation.action_id
            )),
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

#[cfg(target_os = "macos")]
fn send_shortcut(shortcut: &str) -> Result<std::process::Output, String> {
    use hotwire_input_macos::MacEventInjector;
    let injector = MacEventInjector::default();
    let tokens: Vec<&str> = shortcut.split('+').map(str::trim).collect();
    if tokens.is_empty() || tokens.iter().any(|token| token.is_empty()) {
        return Err("shortcut must contain non-empty keys".into());
    }
    for token in &tokens {
        injector
            .key_down_named(token)
            .map_err(|error| format!("could not press {token}: {error}"))?;
    }
    for token in tokens.iter().rev() {
        injector
            .key_up_named(token)
            .map_err(|error| format!("could not release {token}: {error}"))?;
    }
    std::process::Command::new("true")
        .output()
        .map_err(|error| error.to_string())
}

#[cfg(not(target_os = "macos"))]
fn send_shortcut(_shortcut: &str) -> Result<std::process::Output, String> {
    Err("shortcut injection is only available on macOS".into())
}
