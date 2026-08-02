//! Best-effort redaction of secrets and secret-style values from free-form
//! text.
//!
//! Logs and diagnostics must never leak secret values (spec §15.3). A
//! [`Redactor`] masks the literal secret values it was seeded with and any
//! `KEY=value` token whose key looks like a secret (case-insensitive), so a
//! stray `ANTHROPIC_API_KEY=sk-…` pasted into a message is caught even when
//! the exact value was not registered.

use std::collections::BTreeSet;

/// The substitution inserted in place of a redacted value.
pub const REDACTED: &str = "[REDACTED]";

/// Secret-looking assignment keys masked even by a fresh [`Redactor`].
const COMMON_SECRET_KEYS: &[&str] = &[
    "password",
    "passwd",
    "token",
    "secret",
    "api_key",
    "apikey",
    "api-token",
    "authorization",
    "bearer",
];

/// A best-effort text redactor seeded with secret keys and literal values.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Redactor {
    /// Literal secret values replaced verbatim wherever they appear.
    literals: BTreeSet<String>,
    /// Assignment keys (`KEY=value`) whose values are masked case-insensitively.
    keys: BTreeSet<String>,
}

impl Redactor {
    /// Creates a redactor that masks the common secret-looking assignment keys
    /// and any literals you add.
    #[must_use]
    pub fn new() -> Self {
        let keys = COMMON_SECRET_KEYS
            .iter()
            .map(|key| (*key).to_string())
            .collect();
        Self {
            literals: BTreeSet::new(),
            keys,
        }
    }

    /// Registers a literal value to mask everywhere it appears.
    #[must_use]
    pub fn with_literal(mut self, literal: impl Into<String>) -> Self {
        self.add_literal(literal);
        self
    }

    /// Registers a literal value to mask everywhere it appears.
    pub fn add_literal(&mut self, literal: impl Into<String>) {
        let literal = literal.into();
        if !literal.is_empty() {
            self.literals.insert(literal);
        }
    }

    /// Registers an assignment key whose values are masked case-insensitively.
    #[must_use]
    pub fn with_key(mut self, key: impl Into<String>) -> Self {
        self.add_key(key);
        self
    }

    /// Registers an assignment key whose values are masked case-insensitively.
    pub fn add_key(&mut self, key: impl Into<String>) {
        let key = key.into().to_ascii_lowercase();
        if !key.is_empty() {
            self.keys.insert(key);
        }
    }

    /// Redacts `input`, returning a copy in which secrets are masked.
    ///
    /// Every `KEY=value` token whose key is a registered secret key is masked
    /// (value replaced with [`REDACTED`]), then every registered literal value
    /// is masked wherever it appears. The result is intended for logs and
    /// diagnostics; it is not a cryptographic guarantee.
    #[must_use]
    pub fn redact(&self, input: &str) -> String {
        let mut output = input.to_string();
        for key in &self.keys {
            output = mask_assignment(&output, key);
        }
        for literal in &self.literals {
            output = output.replace(literal, REDACTED);
        }
        output
    }

    /// Redacts each input, returning the masked copies.
    #[must_use]
    pub fn redact_many<'a>(&self, inputs: impl IntoIterator<Item = &'a str>) -> Vec<String> {
        inputs.into_iter().map(|input| self.redact(input)).collect()
    }
}

/// Masks every `KEY=` token in `input` where the key matches `key_lower`
/// case-insensitively, preserving the original key's spelling.
fn mask_assignment(input: &str, key_lower: &str) -> String {
    let needle = format!("{key_lower}=");
    let lower = input.to_ascii_lowercase();
    let mut output = String::with_capacity(input.len());
    let mut start = 0;
    while let Some(rel) = lower[start..].find(&needle) {
        let pos = start + rel;
        if token_start(input, pos) {
            output.push_str(&input[start..pos]);
            let key_end = pos + needle.len();
            output.push_str(&input[pos..key_end]);
            output.push_str(REDACTED);
            start = skip_value(input, key_end);
        } else {
            // Not a real assignment (the key is embedded in a larger word);
            // copy it untouched and keep searching after it.
            output.push_str(&input[start..pos + needle.len()]);
            start = pos + needle.len();
        }
    }
    output.push_str(&input[start..]);
    output
}

/// Whether `pos` begins an assignment: the character before it is not a letter
/// or digit, so `monkey=` is never masked by the key `key`, while compound
/// keys like `ANTHROPIC_API_KEY=` (underscore-separated) are.
fn token_start(input: &str, pos: usize) -> bool {
    pos == 0 || !input.as_bytes()[pos - 1].is_ascii_alphanumeric()
}

/// Skips past the value of an assignment, stopping at whitespace or consuming
/// a quoted value in full so the rest of the message survives.
fn skip_value(input: &str, start: usize) -> usize {
    let bytes = input.as_bytes();
    let mut end = start;
    if let Some(&quote @ (b'"' | b'\'')) = bytes.get(end) {
        end += 1;
        while end < bytes.len() && bytes[end] != quote {
            end += 1;
        }
        if end < bytes.len() {
            end += 1; // consume the closing quote
        }
        return end;
    }
    while end < bytes.len() && !is_token_end(bytes[end]) {
        end += 1;
    }
    end
}

fn is_token_end(byte: u8) -> bool {
    byte.is_ascii_whitespace() || byte == b'"' || byte == b'\''
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_secret_assignments_are_masked_by_default() {
        let redactor = Redactor::new();

        assert_eq!(
            redactor.redact("export API_KEY=sk-abc123 then run"),
            "export API_KEY=[REDACTED] then run"
        );
        assert_eq!(redactor.redact("--token=hunter2"), "--token=[REDACTED]");
        assert_eq!(
            redactor.redact("export ANTHROPIC_API_KEY=sk-abc123"),
            "export ANTHROPIC_API_KEY=[REDACTED]"
        );
    }

    #[test]
    fn case_insensitive_keys_are_masked_with_original_spelling() {
        let redactor = Redactor::new().with_key("SESSION_COOKIE");

        assert_eq!(
            redactor.redact("SESSION_COOKIE=abc123"),
            "SESSION_COOKIE=[REDACTED]"
        );
        assert_eq!(
            redactor.redact("session_cookie=abc123 trailing"),
            "session_cookie=[REDACTED] trailing"
        );
    }

    #[test]
    fn literal_secret_values_are_masked_anywhere() {
        let redactor = Redactor::new().with_literal("hunter2");

        let redacted = redactor.redact("the password is hunter2 here");
        assert!(!redacted.contains("hunter2"));
        assert!(redacted.contains("[REDACTED]"));
    }

    #[test]
    fn keys_inside_larger_words_are_not_masked() {
        let redactor = Redactor::new().with_key("key");

        assert_eq!(redactor.redact("monkey=value"), "monkey=value");
    }

    #[test]
    fn quoted_and_spaced_values_are_handled() {
        let redactor = Redactor::new().with_key("TOKEN");

        assert_eq!(
            redactor.redact("TOKEN=\"two words\" done"),
            "TOKEN=[REDACTED] done"
        );
    }

    #[test]
    fn redact_many_applies_to_every_input() {
        let redactor = Redactor::new().with_key("TOKEN");
        let redacted = redactor.redact_many(["TOKEN=a", "TOKEN=b"]);
        assert_eq!(redacted, vec!["TOKEN=[REDACTED]", "TOKEN=[REDACTED]"]);
    }
}
