//! Comet local-daemon adapter.
//!
//! Comet is a peer runtime to Herdr, not a Herdr capability. This adapter talks
//! to Comet's localhost WebSocket RPC and queues semantic agent commands for a
//! configured chat. The active profile chooses either `herdr` or `comet`.

use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use hotwire_adapter_sdk::{
    ActionInvocation, ActionResult, Adapter, AdapterError, AdapterManifest, DetectionResult,
    ValidationResult,
};
use hotwire_core::ActionStatus;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

const DEFAULT_PORT: u16 = 27_654;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CometConfig {
    #[serde(default = "default_port")]
    ipc_port: u16,
    chat_id: Option<String>,
}

impl Default for CometConfig {
    fn default() -> Self {
        Self {
            ipc_port: DEFAULT_PORT,
            chat_id: None,
        }
    }
}

const fn default_port() -> u16 {
    DEFAULT_PORT
}

pub struct CometAdapter {
    manifest: AdapterManifest,
}

impl CometAdapter {
    #[must_use]
    pub fn new() -> Self {
        Self {
            manifest: AdapterManifest {
                id: "comet".into(),
                name: "Comet".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                icon: "comet".into(),
                capabilities: vec![
                    "focus".into(),
                    "new_task".into(),
                    "continue".into(),
                    "plan".into(),
                    "review".into(),
                    "accept".into(),
                    "reject".into(),
                ],
                config_schema: json!({
                    "type": "object",
                    "properties": {
                        "ipcPort": { "type": "integer", "default": DEFAULT_PORT },
                        "chatId": { "type": "string" }
                    }
                }),
            },
        }
    }

    fn parse(config: &Value) -> Result<CometConfig, String> {
        serde_json::from_value(config.clone())
            .map_err(|error| format!("invalid Comet config: {error}"))
    }

    async fn call(
        &self,
        config: &CometConfig,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        let url = format!("ws://127.0.0.1:{}", config.ipc_port);
        let (mut socket, _) = tokio_tungstenite::connect_async(url)
            .await
            .map_err(|error| format!("could not connect to Comet: {error}"))?;
        let frame = json!({ "id": 1, "method": method, "params": params, "cancel": false });
        socket
            .send(Message::Text(frame.to_string().into()))
            .await
            .map_err(|error| format!("could not send Comet RPC: {error}"))?;
        while let Some(message) = socket.next().await {
            let message = message.map_err(|error| format!("Comet RPC failed: {error}"))?;
            if let Message::Text(text) = message {
                let frame: Value = serde_json::from_str(&text)
                    .map_err(|error| format!("invalid Comet response: {error}"))?;
                if let Some(error) = frame.get("err") {
                    return Err(format!("Comet rejected {method}: {error}"));
                }
                if let Some(value) = frame.get("ok") {
                    return Ok(value.clone());
                }
            }
        }
        Err("Comet closed the RPC connection without a response".into())
    }
}

impl Default for CometAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Adapter for CometAdapter {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }

    async fn detect(&self) -> DetectionResult {
        let config = CometConfig::default();
        let detected = self.call(&config, "LocalDevice", json!({})).await.is_ok();
        DetectionResult {
            id: "comet".into(),
            detected,
            version: None,
            path: None,
        }
    }

    async fn validate(&self, config: &Value) -> ValidationResult {
        match Self::parse(config) {
            Ok(config) if config.chat_id.as_deref().is_some_and(|id| !id.is_empty()) => {
                ValidationResult {
                    valid: true,
                    errors: vec![],
                }
            }
            Ok(_) => ValidationResult {
                valid: false,
                errors: vec!["Comet requires a chatId for agent commands".into()],
            },
            Err(error) => ValidationResult {
                valid: false,
                errors: vec![error],
            },
        }
    }

    async fn execute(&self, invocation: &ActionInvocation) -> ActionResult {
        let config = match Self::parse(&invocation.config) {
            Ok(config) => config,
            Err(message) => {
                return ActionResult {
                    execution_id: invocation.execution_id.clone(),
                    status: ActionStatus::Failed,
                    message: Some(message),
                }
            }
        };
        let Some(chat_id) = config.chat_id.clone() else {
            return ActionResult {
                execution_id: invocation.execution_id.clone(),
                status: ActionStatus::Failed,
                message: Some("Comet requires chatId in the binding config".into()),
            };
        };
        let command = match invocation.action_id.as_str() {
            "app.open_or_focus" => "LocalDevice",
            _ => "QueueCommand",
        };
        let params = if command == "LocalDevice" {
            json!({})
        } else {
            json!({ "chatId": chat_id, "command": { "kind": "steer", "prompt": invocation.action_id, "messageId": null } })
        };
        let result = self.call(&config, command, params).await;
        ActionResult {
            execution_id: invocation.execution_id.clone(),
            status: result
                .as_ref()
                .map_or(ActionStatus::Failed, |_| ActionStatus::Succeeded),
            message: result
                .err()
                .or_else(|| Some(format!("Queued through Comet ({})", invocation.action_id))),
        }
    }

    async fn cancel(&self, execution_id: &str) -> Result<(), AdapterError> {
        Err(AdapterError::UnknownExecution(execution_id.into()))
    }
}
