//! Platform seam for the Herdr adapter.
//!
//! The adapter itself is platform-neutral; everything that touches the OS
//! (probing a loopback API, opening a deep link, launching an app, injecting a
//! shortcut) lives behind [`HerdrPlatform`]. Integration tests substitute a
//! mock platform so detection, fallback ordering, and validation are exercised
//! without any real OS side effects.

use std::time::Duration;

use async_trait::async_trait;
use hotwire_adapter_sdk::KeyCombo;
use thiserror::Error;

/// How long a loopback capability probe may take before it is treated as
/// absent. Herdr is machine-local, so a few hundred milliseconds is generous.
pub const API_PROBE_TIMEOUT: Duration = Duration::from_millis(500);

/// Capability path reported by Herdr's local API (spec §13.6).
pub const CAPABILITIES_PATH: &str = "/hotwire/v1/capabilities";
/// Action path prefix Herdr's local API exposes (spec §13.6).
pub const ACTIONS_PATH: &str = "/hotwire/v1/actions";

/// Errors produced by [`HerdrPlatform`] operations.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum HerdrError {
    /// The loopback API was unreachable or answered with an error status.
    #[error("local API error: {0}")]
    Api(String),
    /// A deep link could not be opened.
    #[error("deep link error: {0}")]
    DeepLink(String),
    /// The Herdr application could not be launched or focused.
    #[error("launch error: {0}")]
    Launch(String),
    /// The configured fallback shortcut could not be sent.
    #[error("shortcut error: {0}")]
    Shortcut(String),
    /// The configured shortcut name does not resolve to any physical key.
    #[error("shortcut `{0}` does not resolve to a key")]
    ShortcutResolve(String),
}

/// Everything an adapter needs to talk to a real or mock Herdr integration.
#[async_trait]
pub trait HerdrPlatform: Send + Sync {
    /// Probes `GET /hotwire/v1/capabilities`; returns the reported version
    /// when the API is reachable, `None` otherwise.
    async fn probe_local_api(&self, base_url: &str) -> Option<String>;

    /// Invokes a local API action such as `focus`.
    async fn call_local_api(&self, base_url: &str, action: &str) -> Result<(), HerdrError>;

    /// Opens a `herdr://…` deep link.
    async fn open_deep_link(&self, url: &str) -> Result<(), HerdrError>;

    /// Whether the Herdr application can be found by bundle id or app path.
    fn app_available(&self, bundle_id: Option<&str>, app_path: Option<&str>) -> bool;

    /// Launches or focuses the Herdr application.
    async fn launch_or_focus(
        &self,
        bundle_id: Option<&str>,
        app_path: Option<&str>,
    ) -> Result<(), HerdrError>;

    /// Resolves a shortcut string into a key combo, or `None` when unknown.
    fn resolve_shortcut(&self, shortcut: &str) -> Option<KeyCombo>;

    /// Sends the configured fallback shortcut as a single press.
    async fn send_shortcut(&self, shortcut: &str) -> Result<(), HerdrError>;
}

/// Returns the default platform for the current OS.
///
/// macOS gets a real implementation backed by `open` and Quartz injection;
/// other platforms get a stub so the crate still compiles and reports
/// "not detected".
#[must_use]
pub fn default_platform() -> std::sync::Arc<dyn HerdrPlatform> {
    #[cfg(target_os = "macos")]
    {
        std::sync::Arc::new(macos::MacHerdrPlatform::new())
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::sync::Arc::new(UnsupportedHerdrPlatform)
    }
}

#[cfg(target_os = "macos")]
pub mod macos {
    use std::path::Path;
    use std::process::{Command, Stdio};

    use async_trait::async_trait;
    use hotwire_adapter_sdk::KeyCombo;
    use hotwire_input_macos::{from_physical_name, InjectError, MacEventInjector, INJECTED_MARKER};

    use super::{HerdrError, HerdrPlatform, ACTIONS_PATH, CAPABILITIES_PATH};
    use crate::http::http_request;

    /// Resolves a modifier name to its macOS keycode (left-hand variant).
    #[must_use]
    pub fn resolve_modifier(name: &str) -> Option<u16> {
        match name {
            "shift" => Some(0x38),
            "control" | "ctrl" => Some(0x3B),
            "option" | "alt" => Some(0x3A),
            "command" | "cmd" => Some(0x37),
            "fn" => Some(0x3F),
            _ => None,
        }
    }

    /// A real Herdr platform backed by `open` and Quartz keyboard injection.
    pub struct MacHerdrPlatform {
        injector: MacEventInjector,
    }

    impl MacHerdrPlatform {
        /// Creates a platform that tags injected events as Hotwire's own.
        #[must_use]
        pub fn new() -> Self {
            Self {
                injector: MacEventInjector::new(INJECTED_MARKER),
            }
        }
    }

    impl Default for MacHerdrPlatform {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Drop for MacHerdrPlatform {
        /// Fail-open on process exit: release any key still held so a quit or
        /// crash cannot leave a logical key down (spec §15.5).
        fn drop(&mut self) {
            let _ = self.injector.release_all();
        }
    }

    #[async_trait]
    impl HerdrPlatform for MacHerdrPlatform {
        async fn probe_local_api(&self, base_url: &str) -> Option<String> {
            let response =
                http_request(base_url, "GET", CAPABILITIES_PATH, super::API_PROBE_TIMEOUT).ok()?;
            if !(200..300).contains(&response.status) {
                return None;
            }
            Some(extract_version(&response.body))
        }

        async fn call_local_api(&self, base_url: &str, action: &str) -> Result<(), HerdrError> {
            let path = format!("{ACTIONS_PATH}/{action}");
            let response = http_request(base_url, "POST", &path, super::API_PROBE_TIMEOUT)
                .map_err(HerdrError::Api)?;
            if (200..300).contains(&response.status) {
                Ok(())
            } else {
                Err(HerdrError::Api(format!(
                    "`{path}` returned {}",
                    response.status
                )))
            }
        }

        async fn open_deep_link(&self, url: &str) -> Result<(), HerdrError> {
            run_open(url).map_err(|error| HerdrError::DeepLink(error.to_string()))
        }

        fn app_available(&self, bundle_id: Option<&str>, app_path: Option<&str>) -> bool {
            if let Some(path) = app_path {
                if Path::new(path).exists() {
                    return true;
                }
            }
            let Some(bundle_id) = bundle_id else {
                return false;
            };
            Command::new("mdfind")
                .args(["kMDItemCFBundleIdentifier", "==", &format!("'{bundle_id}'")])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        }

        async fn launch_or_focus(
            &self,
            bundle_id: Option<&str>,
            app_path: Option<&str>,
        ) -> Result<(), HerdrError> {
            let outcome = match (bundle_id, app_path) {
                (_, Some(path)) => run_open(path),
                (Some(bundle_id), None) => run_open(&format!("-a {bundle_id}")),
                (None, None) => {
                    return Err(HerdrError::Launch(
                        "no bundle id or app path configured".to_string(),
                    ))
                }
            };
            outcome.map_err(|error| HerdrError::Launch(error.to_string()))
        }

        fn resolve_shortcut(&self, shortcut: &str) -> Option<KeyCombo> {
            hotwire_adapter_sdk::parse_shortcut(shortcut, resolve_modifier, from_physical_name)
        }

        async fn send_shortcut(&self, shortcut: &str) -> Result<(), HerdrError> {
            let combo = self
                .resolve_shortcut(shortcut)
                .ok_or_else(|| HerdrError::ShortcutResolve(shortcut.to_string()))?;
            for code in &combo.modifiers {
                self.injector
                    .key_down(*code)
                    .map_err(|e| shortcut_error(&e))?;
            }
            self.injector
                .key_down(combo.key)
                .map_err(|e| shortcut_error(&e))?;
            self.injector
                .key_up(combo.key)
                .map_err(|e| shortcut_error(&e))?;
            for code in combo.modifiers.iter().rev() {
                self.injector
                    .key_up(*code)
                    .map_err(|e| shortcut_error(&e))?;
            }
            Ok(())
        }
    }

    fn shortcut_error(error: &InjectError) -> HerdrError {
        HerdrError::Shortcut(error.to_string())
    }

    fn run_open(target: &str) -> Result<(), std::io::Error> {
        let status = Command::new("open").arg(target).status()?;
        if status.success() {
            Ok(())
        } else {
            Err(std::io::Error::other(format!(
                "`open {target}` exited with {status}"
            )))
        }
    }

    fn extract_version(body: &str) -> String {
        serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .and_then(|value| {
                value
                    .get("version")
                    .and_then(serde_json::Value::as_str)
                    .map(ToString::to_string)
            })
            .unwrap_or_else(|| "local".to_string())
    }
}

/// A stub platform used on operating systems without a Herdr integration yet.
///
/// It always reports the API as unreachable, the app as unavailable, and every
/// action as failed, so detection and execution stay explicit everywhere.
#[cfg(not(target_os = "macos"))]
pub struct UnsupportedHerdrPlatform;

#[cfg(not(target_os = "macos"))]
#[async_trait]
impl HerdrPlatform for UnsupportedHerdrPlatform {
    async fn probe_local_api(&self, _base_url: &str) -> Option<String> {
        None
    }

    async fn call_local_api(&self, _base_url: &str, action: &str) -> Result<(), HerdrError> {
        Err(HerdrError::Api(format!(
            "local API is unsupported on this platform (action `{action}`)"
        )))
    }

    async fn open_deep_link(&self, _url: &str) -> Result<(), HerdrError> {
        Err(HerdrError::DeepLink(
            "deep links are unsupported on this platform".to_string(),
        ))
    }

    fn app_available(&self, _bundle_id: Option<&str>, _app_path: Option<&str>) -> bool {
        false
    }

    async fn launch_or_focus(
        &self,
        _bundle_id: Option<&str>,
        _app_path: Option<&str>,
    ) -> Result<(), HerdrError> {
        Err(HerdrError::Launch(
            "app launch is unsupported on this platform".to_string(),
        ))
    }

    fn resolve_shortcut(&self, _shortcut: &str) -> Option<KeyCombo> {
        None
    }

    async fn send_shortcut(&self, shortcut: &str) -> Result<(), HerdrError> {
        Err(HerdrError::Shortcut(format!(
            "shortcut `{shortcut}` is unsupported on this platform"
        )))
    }
}
