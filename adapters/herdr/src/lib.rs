//! Herdr integration adapter (spec §13.6).
//!
//! Herdr is invoked from a physical key as a semantic focus action. The
//! adapter negotiates which integration to use and falls back through a fixed
//! preference order:
//!
//! 1. **Local API / deep link** — a loopback `http://` capability probe, or a
//!    `herdr://…` deep link, when Herdr exposes one.
//! 2. **Application launch / focus** — launch or focus the Herdr macOS app by
//!    bundle id or app path.
//! 3. **Configured shortcut** — reproduce a user-configured shortcut as a
//!    fallback when neither local integration is available.
//!
//! The adapter never assumes the local API exists until a capability probe
//! succeeds (spec §13.6). All OS interaction sits behind [`HerdrPlatform`], so
//! detection, fallback ordering, and validation are exercised in mocked
//! integration tests without real side effects.

mod http;

use std::sync::Arc;

use async_trait::async_trait;
use hotwire_adapter_sdk::{
    ActionInvocation, ActionResult, Adapter, AdapterError, AdapterManifest, DetectionResult,
    ValidationResult,
};
use hotwire_core::ActionStatus;
use serde::Deserialize;
use serde_json::{json, Value};

pub mod platform;
pub use platform::{default_platform, HerdrError, HerdrPlatform};

/// Default loopback Herdr API base URL probed by [`HerdrAdapter::detect`].
pub const DEFAULT_API_BASE_URL: &str = "http://127.0.0.1:7398";

/// Bundle ids probed by [`HerdrAdapter::detect`] when no API is reachable.
pub const DEFAULT_BUNDLE_IDS: &[&str] = &["dev.herdr.app", "Herdr"];

/// Configuration for one Herdr binding (spec §13.6).
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HerdrConfig {
    /// Loopback Herdr API base URL, e.g. `http://127.0.0.1:7398`.
    pub api_base_url: Option<String>,
    /// A `herdr://…` deep link opened with the system handler.
    pub deep_link: Option<String>,
    /// macOS bundle id used to launch or focus the Herdr app.
    pub bundle_id: Option<String>,
    /// Path to the Herdr app bundle when it is not installed in `/Applications`.
    pub app_path: Option<String>,
    /// Fallback shortcut reproduced when no local integration is available.
    pub shortcut: Option<String>,
}

impl HerdrConfig {
    /// Parses an untyped binding config, collecting every field error.
    fn parse(config: &Value) -> Result<Self, String> {
        serde_json::from_value::<Self>(config.clone())
            .map_err(|error| format!("invalid Herdr config: {error}"))
    }

    /// Whether at least one integration path is configured.
    fn has_integration(&self) -> bool {
        self.api_base_url.is_some()
            || self.deep_link.is_some()
            || self.bundle_id.is_some()
            || self.app_path.is_some()
            || self.shortcut.is_some()
    }
}

/// The concrete integration a Herdr execution resolves to.
///
/// Detection is explicit: `negotiate` returns exactly which tier applies, in
/// the preference order of spec §13.6.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HerdrCapability {
    /// The loopback API answered a capability probe.
    LocalApi { base_url: String, version: String },
    /// A `herdr://…` deep link is configured.
    DeepLink { url: String },
    /// The Herdr application is present on this machine.
    App {
        bundle_id: String,
        app_path: Option<String>,
    },
    /// A user-configured shortcut is available as the fallback.
    Shortcut { shortcut: String },
}

/// Focus actions the Herdr adapter executes.
const FOCUS_ACTIONS: &[&str] = &["app.open_or_focus", "herdr.focus"];

/// The Herdr adapter.
pub struct HerdrAdapter {
    manifest: AdapterManifest,
    platform: Arc<dyn HerdrPlatform>,
}

impl HerdrAdapter {
    /// Creates an adapter that talks to Herdr through `platform`.
    #[must_use]
    pub fn new(platform: Arc<dyn HerdrPlatform>) -> Self {
        Self {
            manifest: AdapterManifest {
                id: "herdr".into(),
                name: "Herdr".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                icon: "herdr".into(),
                capabilities: vec![
                    "focus".into(),
                    "new_task".into(),
                    "continue".into(),
                    "review".into(),
                    "accept".into(),
                ],
                config_schema: json!({
                    "type": "object",
                    "properties": {
                        "apiBaseUrl": {
                            "type": "string",
                            "description": "Loopback Herdr API base URL, e.g. http://127.0.0.1:7398"
                        },
                        "deepLink": {
                            "type": "string",
                            "description": "herdr://… deep link opened with the system handler"
                        },
                        "bundleId": {
                            "type": "string",
                            "description": "macOS bundle id used to launch or focus Herdr"
                        },
                        "appPath": {
                            "type": "string",
                            "description": "Path to the Herdr app bundle"
                        },
                        "shortcut": {
                            "type": "string",
                            "description": "Fallback shortcut, e.g. F17 or cmd+shift+h"
                        }
                    }
                }),
            },
            platform,
        }
    }

    /// Resolves the best available integration for `config`.
    ///
    /// Probes in the spec order — local API, then deep link, then app presence,
    /// then the configured shortcut — and returns the first tier that is
    /// available. Returns `None` when no tier is available.
    pub async fn negotiate(&self, config: &HerdrConfig) -> Option<HerdrCapability> {
        if let Some(base_url) = &config.api_base_url {
            if let Some(version) = self.platform.probe_local_api(base_url).await {
                return Some(HerdrCapability::LocalApi {
                    base_url: base_url.clone(),
                    version,
                });
            }
        }
        if let Some(url) = &config.deep_link {
            return Some(HerdrCapability::DeepLink { url: url.clone() });
        }
        if (config.bundle_id.is_some() || config.app_path.is_some())
            && self
                .platform
                .app_available(config.bundle_id.as_deref(), config.app_path.as_deref())
        {
            return Some(HerdrCapability::App {
                bundle_id: config.bundle_id.clone().unwrap_or_default(),
                app_path: config.app_path.clone(),
            });
        }
        if let Some(shortcut) = config
            .shortcut
            .as_deref()
            .filter(|shortcut| self.platform.resolve_shortcut(shortcut).is_some())
        {
            return Some(HerdrCapability::Shortcut {
                shortcut: shortcut.to_string(),
            });
        }
        None
    }
}

#[async_trait]
impl Adapter for HerdrAdapter {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }

    async fn detect(&self) -> DetectionResult {
        // Machine-level presence, independent of any one binding's config:
        // probe the well-known local API, then the common bundle ids.
        if let Some(version) = self.platform.probe_local_api(DEFAULT_API_BASE_URL).await {
            return DetectionResult {
                id: self.manifest.id.clone(),
                detected: true,
                version: Some(version),
                path: None,
            };
        }
        if DEFAULT_BUNDLE_IDS
            .iter()
            .any(|bundle| self.platform.app_available(Some(bundle), None))
        {
            return DetectionResult {
                id: self.manifest.id.clone(),
                detected: true,
                version: None,
                path: None,
            };
        }
        DetectionResult {
            id: self.manifest.id.clone(),
            detected: false,
            version: None,
            path: None,
        }
    }

    async fn validate(&self, config: &Value) -> ValidationResult {
        let parsed = match HerdrConfig::parse(config) {
            Ok(parsed) => parsed,
            Err(message) => {
                return ValidationResult {
                    valid: false,
                    errors: vec![message],
                }
            }
        };

        let mut errors = Vec::new();
        if !parsed.has_integration() {
            errors.push(
                "at least one integration path is required: apiBaseUrl, deepLink, bundleId, appPath, or shortcut"
                    .to_string(),
            );
        }
        if let Some(base_url) = &parsed.api_base_url {
            if !base_url.starts_with("http://") {
                errors.push(format!(
                    "apiBaseUrl must be a plaintext http:// loopback URL, got `{base_url}`"
                ));
            }
        }
        if let Some(deep_link) = &parsed.deep_link {
            if !has_scheme(deep_link) {
                errors.push(format!(
                    "deepLink must include a scheme, e.g. herdr://, got `{deep_link}`"
                ));
            }
        }
        if parsed.bundle_id.as_deref().is_some_and(str::is_empty) {
            errors.push("bundleId must not be empty".to_string());
        }
        if parsed.app_path.as_deref().is_some_and(str::is_empty) {
            errors.push("appPath must not be empty".to_string());
        }
        if let Some(shortcut) = &parsed.shortcut {
            if self.platform.resolve_shortcut(shortcut).is_none() {
                errors.push(format!("shortcut `{shortcut}` does not resolve to a key"));
            }
        }

        ValidationResult {
            valid: errors.is_empty(),
            errors,
        }
    }

    async fn execute(&self, invocation: &ActionInvocation) -> ActionResult {
        let config = match HerdrConfig::parse(&invocation.config) {
            Ok(config) => config,
            Err(message) => return result(invocation, ActionStatus::Failed, Some(message)),
        };
        if !FOCUS_ACTIONS.contains(&invocation.action_id.as_str()) {
            return result(
                invocation,
                ActionStatus::Failed,
                Some(format!(
                    "unsupported action `{}`; Herdr supports {FOCUS_ACTIONS:?}",
                    invocation.action_id
                )),
            );
        }

        // Explicit fallback chain (spec §13.6): each tier is attempted only
        // when it is configured and (for the API) only after a capability probe
        // succeeds; the first tier that succeeds wins.
        let mut attempts: Vec<String> = Vec::new();

        if let Some(base_url) = &config.api_base_url {
            match self.platform.probe_local_api(base_url).await {
                Some(_) => match self.platform.call_local_api(base_url, "focus").await {
                    Ok(()) => {
                        return result(
                            invocation,
                            ActionStatus::Succeeded,
                            Some("Focused Herdr via local API".into()),
                        )
                    }
                    Err(error) => attempts.push(format!("local API: {error}")),
                },
                None => attempts.push("local API unreachable".to_string()),
            }
        }
        if let Some(url) = &config.deep_link {
            match self.platform.open_deep_link(url).await {
                Ok(()) => {
                    return result(
                        invocation,
                        ActionStatus::Succeeded,
                        Some("Opened Herdr via deep link".into()),
                    )
                }
                Err(error) => attempts.push(format!("deep link: {error}")),
            }
        }
        if config.bundle_id.is_some() || config.app_path.is_some() {
            if self
                .platform
                .app_available(config.bundle_id.as_deref(), config.app_path.as_deref())
            {
                match self
                    .platform
                    .launch_or_focus(config.bundle_id.as_deref(), config.app_path.as_deref())
                    .await
                {
                    Ok(()) => {
                        return result(
                            invocation,
                            ActionStatus::Succeeded,
                            Some("Focused Herdr app".into()),
                        )
                    }
                    Err(error) => attempts.push(format!("app launch: {error}")),
                }
            } else {
                attempts.push("Herdr app not found".to_string());
            }
        }
        if let Some(shortcut) = &config.shortcut {
            match self.platform.send_shortcut(shortcut).await {
                Ok(()) => {
                    return result(
                        invocation,
                        ActionStatus::Succeeded,
                        Some("Sent Herdr shortcut".into()),
                    )
                }
                Err(error) => attempts.push(format!("shortcut: {error}")),
            }
        }

        let message = if attempts.is_empty() {
            "no Herdr integration is configured; set apiBaseUrl, deepLink, bundleId/appPath, or shortcut".to_string()
        } else {
            format!("could not focus Herdr: {}", attempts.join("; "))
        };
        result(invocation, ActionStatus::Failed, Some(message))
    }

    async fn cancel(&self, execution_id: &str) -> Result<(), AdapterError> {
        // A Herdr focus execution is instantaneous; there is nothing to cancel.
        Err(AdapterError::UnknownExecution(execution_id.to_string()))
    }
}

fn result(
    invocation: &ActionInvocation,
    status: ActionStatus,
    message: Option<String>,
) -> ActionResult {
    ActionResult {
        execution_id: invocation.execution_id.clone(),
        status,
        message,
    }
}

/// Whether `value` looks like a `scheme://` URL.
fn has_scheme(value: &str) -> bool {
    let Some((scheme, _)) = value.split_once("://") else {
        return false;
    };
    let mut chars = scheme.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic());
    first_ok && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
}
