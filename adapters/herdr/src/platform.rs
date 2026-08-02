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
    use hotwire_input_macos::{from_physical_name, MacEventInjector, INJECTED_MARKER};

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
            // A URL is always a single argv value so spaces and shell
            // metacharacters cannot be reinterpreted.
            run_open(&[url.to_string()]).map_err(|error| HerdrError::DeepLink(error.to_string()))
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
            // `mdfind` exits 0 even when nothing matches, so presence requires
            // both a successful exit and non-empty output.
            Command::new("mdfind")
                .args(["kMDItemCFBundleIdentifier", "==", &format!("'{bundle_id}'")])
                .stderr(Stdio::null())
                .output()
                .is_ok_and(|output| {
                    output.status.success()
                        && !std::str::from_utf8(&output.stdout)
                            .unwrap_or_default()
                            .trim()
                            .is_empty()
                })
        }

        async fn launch_or_focus(
            &self,
            bundle_id: Option<&str>,
            app_path: Option<&str>,
        ) -> Result<(), HerdrError> {
            let args = open_argv(bundle_id, app_path).ok_or_else(|| {
                HerdrError::Launch("no bundle id or app path configured".to_string())
            })?;
            run_open(&args).map_err(|error| HerdrError::Launch(error.to_string()))
        }

        fn resolve_shortcut(&self, shortcut: &str) -> Option<KeyCombo> {
            hotwire_adapter_sdk::parse_shortcut(shortcut, resolve_modifier, from_physical_name)
        }

        async fn send_shortcut(&self, shortcut: &str) -> Result<(), HerdrError> {
            let combo = self
                .resolve_shortcut(shortcut)
                .ok_or_else(|| HerdrError::ShortcutResolve(shortcut.to_string()))?;
            send_combo(&self.injector, &combo)
        }
    }

    /// A minimal key-posting seam so shortcut injection can be exercised
    /// without a real Quartz injector.
    pub(crate) trait ShortcutPoster {
        /// Posts a synthetic key-down.
        fn post_down(&self, keycode: u16) -> Result<(), String>;
        /// Posts a synthetic key-up.
        fn post_up(&self, keycode: u16) -> Result<(), String>;
    }

    impl ShortcutPoster for MacEventInjector {
        fn post_down(&self, keycode: u16) -> Result<(), String> {
            self.key_down(keycode).map_err(|error| error.to_string())
        }

        fn post_up(&self, keycode: u16) -> Result<(), String> {
            self.key_up(keycode).map_err(|error| error.to_string())
        }
    }

    /// Posts a shortcut down and up through `keys`.
    ///
    /// On any partial failure the keys still held are released again in
    /// reverse order (fail-open, spec §15.5), so a key-down or key-up error can
    /// never leave a logical key held. Successfully released keys are dropped
    /// from the tracked set so a later-up failure never re-posts a duplicate
    /// key-up.
    fn send_combo(keys: &impl ShortcutPoster, combo: &KeyCombo) -> Result<(), HerdrError> {
        let mut posted = Vec::new();
        for code in combo.all_keycodes() {
            match keys.post_down(code) {
                Ok(()) => posted.push(code),
                Err(error) => {
                    release_posted(keys, &posted);
                    return Err(shortcut_error_string(error));
                }
            }
        }
        for code in combo.release_order() {
            if let Err(error) = keys.post_up(code) {
                release_posted(keys, &posted);
                return Err(shortcut_error_string(error));
            }
            posted.retain(|held| *held != code);
        }
        Ok(())
    }

    /// Best-effort release of every posted key, in reverse order.
    fn release_posted(keys: &impl ShortcutPoster, posted: &[u16]) {
        for code in posted.iter().rev() {
            let _ = keys.post_up(*code);
        }
    }

    /// The argv for `/usr/bin/open`, modeled structurally so a path or URL
    /// stays a single safe argument and a bundle id is passed as `-b <id>`.
    #[must_use]
    fn open_argv(bundle_id: Option<&str>, app_path: Option<&str>) -> Option<Vec<String>> {
        match (bundle_id, app_path) {
            (_, Some(path)) => Some(vec![path.to_string()]),
            (Some(bundle_id), None) => Some(vec!["-b".to_string(), bundle_id.to_string()]),
            (None, None) => None,
        }
    }

    fn shortcut_error_string(error: String) -> HerdrError {
        HerdrError::Shortcut(error)
    }

    fn run_open(args: &[String]) -> Result<(), std::io::Error> {
        let status = Command::new("open").args(args).status()?;
        if status.success() {
            Ok(())
        } else {
            Err(std::io::Error::other(format!(
                "`open {}` exited with {status}",
                args.join(" ")
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

    #[cfg(test)]
    mod tests {
        use std::cell::{Cell, RefCell};

        use hotwire_adapter_sdk::KeyCombo;

        use super::{open_argv, send_combo, MacHerdrPlatform, ShortcutPoster};
        use crate::platform::HerdrPlatform;

        /// A poster that records every call and fails once on a chosen
        /// 1-indexed down or up call, so partial-injection failures can be
        /// reproduced deterministically.
        struct FailingPoster {
            downs: RefCell<Vec<u16>>,
            ups: RefCell<Vec<u16>>,
            fail_down_on: Cell<Option<usize>>,
            fail_up_on: Cell<Option<usize>>,
        }

        impl FailingPoster {
            fn new() -> Self {
                Self {
                    downs: RefCell::new(Vec::new()),
                    ups: RefCell::new(Vec::new()),
                    fail_down_on: Cell::new(None),
                    fail_up_on: Cell::new(None),
                }
            }

            fn fail_down_on(self, call: usize) -> Self {
                self.fail_down_on.set(Some(call));
                self
            }

            fn fail_up_on(self, call: usize) -> Self {
                self.fail_up_on.set(Some(call));
                self
            }
        }

        impl ShortcutPoster for FailingPoster {
            fn post_down(&self, keycode: u16) -> Result<(), String> {
                let next = self.downs.borrow().len() + 1;
                if self.fail_down_on.get() == Some(next) {
                    self.fail_down_on.set(None);
                    return Err(format!("down {next} failed"));
                }
                self.downs.borrow_mut().push(keycode);
                Ok(())
            }

            fn post_up(&self, keycode: u16) -> Result<(), String> {
                let next = self.ups.borrow().len() + 1;
                if self.fail_up_on.get() == Some(next) {
                    self.fail_up_on.set(None);
                    return Err(format!("up {next} failed"));
                }
                self.ups.borrow_mut().push(keycode);
                Ok(())
            }
        }

        #[test]
        fn open_argv_models_bundle_id_and_path_separately() {
            assert_eq!(
                open_argv(Some("dev.herdr.app"), None),
                Some(vec!["-b".to_string(), "dev.herdr.app".to_string()])
            );
            // A path stays one argv value even with spaces.
            assert_eq!(
                open_argv(None, Some("/Applications/My Herdr.app")),
                Some(vec!["/Applications/My Herdr.app".to_string()])
            );
            assert_eq!(open_argv(None, None), None);
        }

        #[test]
        fn mdfind_requires_non_empty_output_for_app_presence() {
            // A bundle id that cannot exist must never report as installed,
            // even though `mdfind` exits 0 with no matches.
            let platform = MacHerdrPlatform::new();
            assert!(!platform
                .app_available(Some("com.hotwire.definitely-not-an-installed-bundle"), None));
        }

        #[test]
        fn send_combo_releases_posted_keys_in_reverse_on_partial_down_failure() {
            let combo = KeyCombo {
                modifiers: vec![0x3F, 0x37],
                key: 0x31,
            };
            let poster = FailingPoster::new().fail_down_on(3); // the key itself fails

            let result = send_combo(&poster, &combo);
            assert!(matches!(
                result,
                Err(crate::platform::HerdrError::Shortcut(_))
            ));
            assert_eq!(poster.downs.borrow().as_slice(), &[0x3F, 0x37]);
            assert_eq!(
                poster.ups.borrow().as_slice(),
                &[0x37, 0x3F],
                "modifiers already posted are released in reverse"
            );
        }

        #[test]
        fn send_combo_releases_everything_on_a_partial_up_failure() {
            let combo = KeyCombo {
                modifiers: vec![0x3F, 0x37],
                key: 0x31,
            };
            // The first key-up (reverse order: key first) fails.
            let poster = FailingPoster::new().fail_up_on(1);

            let result = send_combo(&poster, &combo);
            assert!(result.is_err());
            assert_eq!(poster.downs.borrow().as_slice(), &[0x3F, 0x37, 0x31]);
            assert_eq!(
                poster.ups.borrow().as_slice(),
                &[0x31, 0x37, 0x3F],
                "every posted key is released in reverse"
            );
        }

        #[test]
        fn send_combo_succeeds_and_releases_in_reverse_order() {
            let combo = KeyCombo {
                modifiers: vec![0x3F, 0x37],
                key: 0x31,
            };
            let poster = FailingPoster::new();

            assert!(send_combo(&poster, &combo).is_ok());
            assert_eq!(poster.downs.borrow().as_slice(), &[0x3F, 0x37, 0x31]);
            assert_eq!(poster.ups.borrow().as_slice(), &[0x31, 0x37, 0x3F]);
        }

        #[test]
        fn send_combo_never_posts_duplicate_ups_on_a_later_up_failure() {
            let combo = KeyCombo {
                modifiers: vec![0x3F, 0x37],
                key: 0x31,
            };
            // The key (first up) succeeds and is dropped from the tracked set;
            // the second up (0x37) fails, so only 0x37 and 0x3F are still held
            // and must be released — never a duplicate 0x31.
            let poster = FailingPoster::new().fail_up_on(2);

            let result = send_combo(&poster, &combo);
            assert!(result.is_err());
            assert_eq!(poster.downs.borrow().as_slice(), &[0x3F, 0x37, 0x31]);
            assert_eq!(
                poster.ups.borrow().as_slice(),
                &[0x31, 0x37, 0x3F],
                "every key released exactly once"
            );
        }
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
