//! Sanitized command environments.
//!
//! Commands never inherit the host environment wholesale. A [`SanitizedEnv`]
//! is rebuilt from scratch: an explicit set of variables plus a named
//! allowlist of host variables to carry over. Keys that hold secrets are
//! tracked so their values can be redacted from every log and diagnostic
//! surface (spec §15.3).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::command::CommandError;
use crate::redact::Redactor;

/// A set of environment variable names.
pub type SecretSet = BTreeSet<String>;

/// The value substituted for secret values in redacted output.
const REDACTED_VALUE: &str = "[REDACTED]";

/// A sanitized environment for one command execution.
///
/// `Eq`/`Ord` are full structural equality over the resolved plan, which is
/// part of what approval is bound to. `Debug` is redacted: the value of any
/// marked secret key is masked, so derived output never leaks a secret value.
#[derive(Clone, Default, Eq, PartialEq, PartialOrd, Ord)]
pub struct SanitizedEnv {
    /// Variables explicitly set for the command; these always win.
    explicit: BTreeMap<String, String>,
    /// Names of host environment variables the command may inherit. Anything
    /// not listed is stripped from the child.
    inherit: BTreeSet<String>,
    /// Names of variables whose values must be redacted from any output that
    /// leaves the runner (logs, diagnostics, receipts).
    secrets: SecretSet,
}

impl fmt::Debug for SanitizedEnv {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SanitizedEnv")
            .field("explicit", &self.build_redacted())
            .field("inherit", &self.inherit)
            .field("secrets", &self.secrets)
            .finish()
    }
}

impl SanitizedEnv {
    /// Creates an empty environment: nothing inherited, nothing explicit.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets an explicit variable, returning `self` for chaining.
    #[must_use]
    pub fn with_var(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.set_var(key, value);
        self
    }

    /// Sets an explicit variable.
    pub fn set_var(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.explicit.insert(key.into(), value.into());
    }

    /// Allows the child to inherit a named host variable, returning `self`.
    #[must_use]
    pub fn inherit(mut self, key: impl Into<String>) -> Self {
        self.inherit.insert(key.into());
        self
    }

    /// Allows the child to inherit each named host variable.
    #[must_use]
    pub fn inherit_many(mut self, keys: impl IntoIterator<Item = String>) -> Self {
        self.inherit.extend(keys);
        self
    }

    /// Marks `key` as a secret whose value is redacted from any log or
    /// diagnostic output.
    pub fn mark_secret(&mut self, key: impl Into<String>) {
        self.secrets.insert(key.into());
    }

    /// Returns whether `key` is tracked as a secret.
    #[must_use]
    pub fn is_secret(&self, key: &str) -> bool {
        self.secrets.contains(key)
    }

    /// Returns the names of all secret variables.
    #[must_use]
    pub fn secret_keys(&self) -> &SecretSet {
        &self.secrets
    }

    /// Returns the explicit variable names.
    #[must_use]
    pub fn var_names(&self) -> Vec<String> {
        self.explicit.keys().cloned().collect()
    }

    /// Builds the child environment: allowed host variables plus explicit
    /// variables (explicit wins).
    #[must_use]
    pub fn build(&self) -> BTreeMap<String, String> {
        let mut env = BTreeMap::new();
        for key in &self.inherit {
            if let Some(value) = std::env::var_os(key) {
                env.insert(key.clone(), value.to_string_lossy().into_owned());
            }
        }
        for (key, value) in &self.explicit {
            env.insert(key.clone(), value.clone());
        }
        env
    }

    /// Like [`SanitizedEnv::build`], but every secret value is masked.
    ///
    /// This is what logs and diagnostics may show; it never reveals a secret
    /// value.
    #[must_use]
    pub fn build_redacted(&self) -> BTreeMap<String, String> {
        self.build()
            .into_iter()
            .map(|(key, value)| {
                if self.secrets.contains(&key) {
                    (key, REDACTED_VALUE.to_string())
                } else {
                    (key, value)
                }
            })
            .collect()
    }

    /// Returns a [`Redactor`] seeded from the *resolved* sanitized environment,
    /// ready to mask the same values inside free-form messages.
    ///
    /// This covers secrets from both sources: explicit variables and values
    /// inherited from the host for marked keys.
    #[must_use]
    pub fn redactor(&self) -> Redactor {
        self.redactor_for(&self.build())
    }

    /// Returns a [`Redactor`] seeded from a resolved environment map.
    ///
    /// `resolved` is the environment the child actually receives (usually
    /// [`SanitizedEnv::build`]); for every marked secret key its value there —
    /// explicit or inherited — is registered as a literal to mask.
    #[must_use]
    pub fn redactor_for(&self, resolved: &BTreeMap<String, String>) -> Redactor {
        let mut redactor = Redactor::new();
        for key in &self.secrets {
            redactor = redactor.with_key(key.clone());
            if let Some(value) = resolved.get(key) {
                redactor = redactor.with_literal(value.clone());
            }
        }
        redactor
    }

    /// Validates that every key is a legal environment variable name.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError::InvalidEnvironment`] when a key is empty or
    /// contains `=` or a NUL byte.
    pub fn validate(&self) -> Result<(), CommandError> {
        for key in self
            .explicit
            .keys()
            .chain(self.inherit.iter())
            .chain(self.secrets.iter())
        {
            if key.is_empty() || key.contains('=') || key.contains('\0') {
                return Err(CommandError::InvalidEnvironment(key.clone()));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_overlays_explicit_vars_on_an_allowlist() {
        let env = SanitizedEnv::new()
            .with_var("HOTWIRE_MODE", "safe")
            .inherit("PATH")
            .inherit("HOME");

        let built = env.build();
        assert_eq!(built.get("HOTWIRE_MODE").map(String::as_str), Some("safe"));
        // Allowlisted host vars that happen to exist are carried over.
        assert_eq!(built.get("PATH"), std::env::var("PATH").ok().as_ref());
        assert_eq!(built.get("HOME"), std::env::var("HOME").ok().as_ref());
        // Everything else is stripped.
        for key in std::env::vars().map(|(key, _)| key) {
            if !["HOTWIRE_MODE", "PATH", "HOME"].contains(&key.as_str()) {
                assert!(!built.contains_key(&key));
            }
        }
    }

    #[test]
    fn explicit_vars_win_over_inherited_host_values() {
        let env = SanitizedEnv::new().with_var("HOME", "/sandbox");
        assert_eq!(
            env.build().get("HOME").map(String::as_str),
            Some("/sandbox")
        );
    }

    #[test]
    fn secret_values_are_masked_in_redacted_builds() {
        let mut env = SanitizedEnv::new().with_var("ANTHROPIC_API_KEY", "sk-secret-123");
        env.mark_secret("ANTHROPIC_API_KEY");
        env.set_var("USER", "brede");

        let redacted = env.build_redacted();
        assert_eq!(
            redacted.get("ANTHROPIC_API_KEY").map(String::as_str),
            Some("[REDACTED]")
        );
        assert_eq!(redacted.get("USER").map(String::as_str), Some("brede"));
    }

    #[test]
    fn redactor_is_seeded_with_secret_keys_and_values() {
        let mut env = SanitizedEnv::new().with_var("TOKEN", "hunter2");
        env.mark_secret("TOKEN");

        let redactor = env.redactor();
        assert!(redactor
            .redact("export TOKEN=hunter2")
            .contains("[REDACTED]"));
        assert!(!redactor.redact("export TOKEN=hunter2").contains("hunter2"));
    }

    #[test]
    fn redactor_covers_secrets_inherited_from_the_resolved_environment() {
        let mut env = SanitizedEnv::new().inherit("HOST_SECRET");
        env.mark_secret("HOST_SECRET");
        // The resolved environment is what the child actually receives; a
        // marked secret inherited from the host is only visible there.
        let mut resolved = env.build();
        resolved.insert("HOST_SECRET".into(), "host-inherited-value".into());

        let redactor = env.redactor_for(&resolved);
        let masked = redactor.redact("the child echoed host-inherited-value");
        assert!(
            !masked.contains("host-inherited-value"),
            "an inherited secret value must be masked"
        );
        assert!(masked.contains("[REDACTED]"));
    }

    #[test]
    fn debug_output_masks_secret_values_but_keeps_others() {
        let mut env = SanitizedEnv::new().with_var("API_TOKEN", "ghp-super-secret");
        env.mark_secret("API_TOKEN");

        let debug = format!("{env:?}");
        assert!(!debug.contains("ghp-super-secret"));
        assert!(debug.contains("[REDACTED]"));

        let plain = SanitizedEnv::new().with_var("HOTWIRE_MODE", "safe");
        assert!(format!("{plain:?}").contains("safe"));
    }

    #[test]
    fn validate_rejects_illegal_keys() {
        let empty = SanitizedEnv::new().with_var("", "x");
        assert!(matches!(
            empty.validate(),
            Err(CommandError::InvalidEnvironment(_))
        ));

        let with_equals = SanitizedEnv::new().with_var("A=B", "x");
        assert!(matches!(
            with_equals.validate(),
            Err(CommandError::InvalidEnvironment(_))
        ));
    }
}
