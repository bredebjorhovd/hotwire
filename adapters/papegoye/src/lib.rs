//! Papegøye voice-input adapter (spec §13.5).
//!
//! Papegøye is a separate application that owns microphone capture,
//! transcription, and text insertion. Hotwire is responsible only for invoking
//! the voice interaction from a physical key — **no microphone data belongs in
//! Hotwire**. This adapter reproduces Papegøye's configured push-to-talk
//! shortcut as a true hold:
//!
//! ```text
//! physical key down → Papegøye shortcut key down
//! physical key held → no repeated invocation
//! physical key up   → Papegøye shortcut key up
//! ```
//!
//! The hold state machine is reference-counted per keycode so a down is always
//! paired with exactly one up, even when executions overlap or are cancelled
//! during shutdown — no key can be left logically stuck down.

mod platform;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use hotwire_adapter_sdk::{
    parse_shortcut, ActionInvocation, ActionResult, Adapter, AdapterError, AdapterManifest,
    DetectionResult, KeyCombo, ValidationResult,
};
use hotwire_core::{ActionStatus, Trigger};
use serde::Deserialize;
use serde_json::{json, Value};

pub use platform::{default_platform, PapegoyeError, PapegoyePlatform};

/// Configuration for one Papegøye binding (spec §13.5).
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PapegoyeConfig {
    /// Human-readable push-to-talk shortcut, e.g. `"fn+space"` or `"F17"`.
    pub shortcut: Option<String>,
    /// Explicit platform keycode, alternative to `shortcut`.
    pub keycode: Option<u16>,
    /// Modifier names applied with `keycode` (e.g. `["fn"]`).
    #[serde(default)]
    pub modifiers: Vec<String>,
}

impl PapegoyeConfig {
    /// Parses an untyped binding config, collecting every field error.
    fn parse(config: &Value) -> Result<Self, String> {
        serde_json::from_value::<Self>(config.clone())
            .map_err(|error| format!("invalid Papegøye config: {error}"))
    }
}

/// The Papegøye adapter.
pub struct PapegoyeAdapter {
    manifest: AdapterManifest,
    platform: Arc<dyn PapegoyePlatform>,
    /// execution id → the key combo that execution is currently holding.
    active: Mutex<HashMap<String, KeyCombo>>,
    /// keycode → how many active executions hold it; a down/up is posted only
    /// on the 0→1 and 1→0 transitions so repeats never occur.
    held: Mutex<HashMap<u16, u32>>,
}

impl PapegoyeAdapter {
    /// Creates an adapter that reproduces push-to-talk through `platform`.
    #[must_use]
    pub fn new(platform: Arc<dyn PapegoyePlatform>) -> Self {
        Self {
            manifest: AdapterManifest {
                id: "papegoye".into(),
                name: "Papegøye".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                icon: "papegoye".into(),
                capabilities: vec!["start".into(), "stop".into(), "cancel".into()],
                config_schema: json!({
                    "type": "object",
                    "properties": {
                        "shortcut": {
                            "type": "string",
                            "description": "Papegøye's configured push-to-talk shortcut, e.g. fn+space or F17"
                        },
                        "keycode": {
                            "type": "number",
                            "description": "Explicit platform keycode, alternative to `shortcut`"
                        },
                        "modifiers": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Modifier names applied with `keycode`"
                        }
                    }
                }),
            },
            platform,
            active: Mutex::new(HashMap::new()),
            held: Mutex::new(HashMap::new()),
        }
    }

    /// Resolves the configured shortcut or keycode into a [`KeyCombo`].
    fn resolve_combo(&self, config: &PapegoyeConfig) -> Result<KeyCombo, String> {
        if let Some(shortcut) = &config.shortcut {
            parse_shortcut(
                shortcut,
                |name| self.platform.resolve_modifier(name),
                |name| self.platform.resolve_key(name),
            )
            .ok_or_else(|| format!("shortcut `{shortcut}` does not resolve to a key"))
        } else {
            let keycode = config
                .keycode
                .ok_or_else(|| "config must set `shortcut` or `keycode`".to_string())?;
            if keycode == 0 {
                return Err("keycode must be non-zero".to_string());
            }
            let mut modifiers = Vec::with_capacity(config.modifiers.len());
            for name in &config.modifiers {
                modifiers.push(
                    self.platform
                        .resolve_modifier(name)
                        .ok_or_else(|| format!("unknown modifier `{name}`"))?,
                );
            }
            Ok(KeyCombo {
                modifiers,
                key: keycode,
            })
        }
    }

    /// Ends a hold for `execution_id`, posting exactly one key-up per keycode
    /// the execution holds. Shared keycodes only release when the last holder
    /// releases, so an up is never posted twice.
    async fn end_hold(&self, execution_id: &str) -> Result<(), AdapterError> {
        let combo = self
            .active
            .lock()
            .expect("active lock")
            .remove(execution_id)
            .ok_or_else(|| AdapterError::UnknownExecution(execution_id.to_string()))?;

        let to_release = {
            let mut held = self.held.lock().expect("held lock");
            let mut to_release = Vec::new();
            for code in combo.all_keycodes() {
                if let Some(count) = held.get_mut(&code) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        to_release.push(code);
                    }
                }
            }
            to_release
        };
        for code in to_release {
            let _ = self.platform.key_up(code).await;
        }
        Ok(())
    }

    /// Posts a key-down for `combo`, tracking it so the release is exact.
    ///
    /// The execution is reserved atomically (before any `await`), so two
    /// concurrent starts of the same execution id cannot both observe it
    /// absent and double-book the reference counts. Keycodes already held by
    /// another execution are skipped so a repeated down is never posted. On a
    /// partial failure the reservation is rolled back and the successfully
    /// posted downs are released again (fail-open).
    async fn start_hold(&self, execution_id: &str, combo: &KeyCombo) -> ActionResult {
        let to_down = {
            let mut active = self.active.lock().expect("active lock");
            if active.contains_key(execution_id) {
                return start_result(execution_id);
            }
            let mut held = self.held.lock().expect("held lock");
            let mut to_down = Vec::new();
            for code in combo.all_keycodes() {
                let count = held.entry(code).or_insert(0);
                *count += 1;
                if *count == 1 {
                    to_down.push(code);
                }
            }
            active.insert(execution_id.to_string(), combo.clone());
            to_down
        };

        let mut posted = Vec::new();
        for code in &to_down {
            match self.platform.key_down(*code).await {
                Ok(()) => posted.push(*code),
                Err(error) => {
                    self.roll_back_reservation(execution_id, combo, &posted)
                        .await;
                    return failed_result(
                        execution_id,
                        format!("could not press the push-to-talk key: {error}"),
                    );
                }
            }
        }

        start_result(execution_id)
    }

    /// Removes the reservation made by [`PapegoyeAdapter::start_hold`] and
    /// releases exactly the keys this execution had already posted.
    async fn roll_back_reservation(&self, execution_id: &str, combo: &KeyCombo, posted: &[u16]) {
        {
            let mut active = self.active.lock().expect("active lock");
            active.remove(execution_id);
        }
        {
            let mut held = self.held.lock().expect("held lock");
            for code in combo.all_keycodes() {
                if let Some(count) = held.get_mut(&code) {
                    *count = count.saturating_sub(1);
                }
            }
        }
        for code in posted.iter().rev() {
            let _ = self.platform.key_up(*code).await;
        }
    }

    /// Sends a complete press (down then up) for a `press`-triggered binding.
    async fn tap(&self, execution_id: &str, combo: &KeyCombo) -> ActionResult {
        let mut posted = Vec::new();
        for code in combo.all_keycodes() {
            match self.platform.key_down(code).await {
                Ok(()) => posted.push(code),
                Err(error) => {
                    for code in &posted {
                        let _ = self.platform.key_up(*code).await;
                    }
                    return failed_result(
                        execution_id,
                        format!("could not press the push-to-talk key: {error}"),
                    );
                }
            }
        }
        for code in posted.iter().rev() {
            let _ = self.platform.key_up(*code).await;
        }
        ActionResult {
            execution_id: execution_id.to_string(),
            status: ActionStatus::Succeeded,
            message: Some("Pressed Papegøye push-to-talk".into()),
        }
    }

    /// Releases every key any execution is holding and forgets all executions.
    ///
    /// Called on shutdown so no logical key is left down after Hotwire quits
    /// or crashes (fail-open invariant).
    ///
    /// # Panics
    ///
    /// Panics if the internal lock is poisoned by a panic in another thread.
    #[must_use]
    pub async fn release_all(&self) -> Vec<u16> {
        self.active.lock().expect("active lock").clear();
        let held: Vec<u16> = self
            .held
            .lock()
            .expect("held lock")
            .drain()
            .map(|(keycode, _)| keycode)
            .collect();
        for keycode in &held {
            let _ = self.platform.key_up(*keycode).await;
        }
        held
    }
}

#[async_trait]
impl Adapter for PapegoyeAdapter {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }

    async fn detect(&self) -> DetectionResult {
        // Papegøye presence is a pure app-install check (spec §13.5). The
        // adapter never probes for, captures, or retains microphone audio —
        // no microphone data belongs in Hotwire.
        DetectionResult {
            id: self.manifest.id.clone(),
            detected: self.platform.app_available(),
            version: None,
            path: None,
        }
    }

    async fn validate(&self, config: &Value) -> ValidationResult {
        let parsed = match PapegoyeConfig::parse(config) {
            Ok(parsed) => parsed,
            Err(message) => {
                return ValidationResult {
                    valid: false,
                    errors: vec![message],
                }
            }
        };

        let mut errors = Vec::new();
        match (&parsed.shortcut, parsed.keycode) {
            (None, None) => errors.push("config must set `shortcut` or `keycode`".to_string()),
            (Some(_), Some(_)) => {
                errors.push("config must set exactly one of `shortcut` or `keycode`".to_string());
            }
            (Some(shortcut), None) => {
                if let Err(message) = self.resolve_combo(&parsed) {
                    errors.push(message);
                }
                if shortcut.is_empty() {
                    errors.push("shortcut must not be empty".to_string());
                }
            }
            (None, Some(keycode)) => {
                if keycode == 0 {
                    errors.push("keycode must be non-zero".to_string());
                }
                if let Err(message) = self.resolve_combo(&parsed) {
                    errors.push(message);
                }
            }
        }

        ValidationResult {
            valid: errors.is_empty(),
            errors,
        }
    }

    async fn execute(&self, invocation: &ActionInvocation) -> ActionResult {
        if invocation.action_id != "voice.input" {
            return failed_result(
                &invocation.execution_id,
                format!(
                    "unsupported action `{}`; Papegøye executes `voice.input`",
                    invocation.action_id
                ),
            );
        }
        let config = match PapegoyeConfig::parse(&invocation.config) {
            Ok(config) => config,
            Err(message) => return failed_result(&invocation.execution_id, message),
        };
        let combo = match self.resolve_combo(&config) {
            Ok(combo) => combo,
            Err(message) => return failed_result(&invocation.execution_id, message),
        };

        match invocation.trigger {
            Trigger::Hold => self.start_hold(&invocation.execution_id, &combo).await,
            Trigger::Press => self.tap(&invocation.execution_id, &combo).await,
            Trigger::DoublePress => failed_result(
                &invocation.execution_id,
                "voice.input only supports `hold` (push-to-talk) and `press`".to_string(),
            ),
        }
    }

    async fn cancel(&self, execution_id: &str) -> Result<(), AdapterError> {
        // Cancellation (e.g. runtime shutdown) must release the shortcut so no
        // key is left logically held down.
        self.end_hold(execution_id).await
    }

    async fn release(&self, execution_id: &str) -> Result<(), AdapterError> {
        self.end_hold(execution_id).await
    }
}

fn start_result(execution_id: &str) -> ActionResult {
    ActionResult {
        execution_id: execution_id.to_string(),
        status: ActionStatus::Started,
        message: Some("Held Papegøye push-to-talk".into()),
    }
}

fn failed_result(execution_id: &str, message: String) -> ActionResult {
    ActionResult {
        execution_id: execution_id.to_string(),
        status: ActionStatus::Failed,
        message: Some(message),
    }
}
