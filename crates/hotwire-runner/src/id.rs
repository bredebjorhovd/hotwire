//! Validated identifier newtypes.
//!
//! The persistent-log guarantee ("no typed text, prompts, paths, or key
//! sequences") is upheld structurally: every string-bearing log field is a
//! validated identifier newtype, so arbitrary caller text cannot be
//! represented. Review ids are internally generated and validated.

use std::fmt;

use thiserror::Error;

/// Error returned when an identifier fails validation.
#[derive(Debug, Error)]
pub enum IdentifierError {
    /// The value is not a valid identifier.
    #[error("`{0}` is not a valid identifier (allowed: ASCII letters, digits, `.`, `_`, `-`)")]
    Invalid(String),
}

/// Whether `value` is a valid identifier: non-empty ASCII letters/digits plus
/// `.`, `_`, and `-`. Prompts, paths, and key sequences are rejected.
#[must_use]
pub fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// An action id such as `shell.run` or `app.open_or_focus`.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct ActionId(String);

impl ActionId {
    /// Validates and wraps `value`.
    ///
    /// # Errors
    ///
    /// Returns [`IdentifierError::Invalid`] when `value` is empty or contains
    /// characters outside ASCII letters, digits, `.`, `_`, and `-`.
    pub fn try_new(value: impl Into<String>) -> Result<Self, IdentifierError> {
        let value = value.into();
        if !valid_identifier(&value) {
            return Err(IdentifierError::Invalid(value));
        }
        Ok(Self(value))
    }

    /// Returns the underlying identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An adapter id such as `herdr` or `shell`.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct AdapterId(String);

impl AdapterId {
    /// Validates and wraps `value`.
    ///
    /// # Errors
    ///
    /// Returns [`IdentifierError::Invalid`] when `value` is empty or contains
    /// characters outside ASCII letters, digits, `.`, `_`, and `-`.
    pub fn try_new(value: impl Into<String>) -> Result<Self, IdentifierError> {
        let value = value.into();
        if !valid_identifier(&value) {
            return Err(IdentifierError::Invalid(value));
        }
        Ok(Self(value))
    }

    /// Returns the underlying identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A single configured physical code such as `Numpad5` or `KeyA`.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct PhysicalCode(String);

impl PhysicalCode {
    /// Validates and wraps `value`.
    ///
    /// # Errors
    ///
    /// Returns [`IdentifierError::Invalid`] when `value` is empty or contains
    /// characters outside ASCII letters, digits, `.`, `_`, and `-`.
    pub fn try_new(value: impl Into<String>) -> Result<Self, IdentifierError> {
        let value = value.into();
        if !valid_identifier(&value) {
            return Err(IdentifierError::Invalid(value));
        }
        Ok(Self(value))
    }

    /// Returns the underlying physical code.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A review id (`review-<n>`), generated internally by the approval store.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct ReviewId(String);

impl ReviewId {
    /// Validates and wraps a `review-<digits>` id.
    ///
    /// # Errors
    ///
    /// Returns [`IdentifierError::Invalid`] when `value` is not of the form
    /// `review-<digits>`.
    pub fn try_new(value: impl Into<String>) -> Result<Self, IdentifierError> {
        let value = value.into();
        let valid = value
            .strip_prefix("review-")
            .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()));
        if !valid {
            return Err(IdentifierError::Invalid(value));
        }
        Ok(Self(value))
    }

    /// Returns the underlying review id.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ActionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl fmt::Display for AdapterId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl fmt::Display for PhysicalCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl fmt::Display for ReviewId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_identifiers_are_accepted() {
        for value in [
            "app.open_or_focus",
            "shell.run",
            "Numpad5",
            "KeyA",
            "herdr",
            "x-1",
        ] {
            assert!(ActionId::try_new(value).is_ok(), "{value} must be valid");
            assert!(
                PhysicalCode::try_new(value).is_ok(),
                "{value} must be valid"
            );
        }
    }

    #[test]
    fn prompts_paths_and_key_sequences_are_rejected() {
        for value in [
            "hello world",
            "/Users/brede/secret/path",
            "what is your name?",
            "Numpad5 Numpad0 Numpad1",
            "key\nsequence",
            "prompt: run the command",
            "",
        ] {
            assert!(
                ActionId::try_new(value).is_err(),
                "action id {value:?} must be rejected"
            );
            assert!(
                AdapterId::try_new(value).is_err(),
                "adapter id {value:?} must be rejected"
            );
            assert!(
                PhysicalCode::try_new(value).is_err(),
                "physical code {value:?} must be rejected"
            );
        }
    }

    #[test]
    fn review_ids_are_internally_formed_and_validated() {
        assert!(ReviewId::try_new("review-1").is_ok());
        assert!(ReviewId::try_new("review-42").is_ok());
        assert!(ReviewId::try_new("review-").is_err());
        assert!(ReviewId::try_new("1").is_err());
        assert!(ReviewId::try_new("review-x").is_err());
        assert!(ReviewId::try_new("delete /tmp").is_err());
    }
}
