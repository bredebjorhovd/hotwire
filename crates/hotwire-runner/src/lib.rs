//! Command execution boundary.
//!
//! The runner turns shell and script action configs into inspectable,
//! cancellable executions with timeouts. The actual process spawning and
//! cancellation machinery lands with SAFE-001; this crate establishes the
//! *review-before-execute* and *cancellation* boundaries that make execution
//! safe. Imported shell actions must expose their exact command before first
//! execution, and never run from the input callback thread.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;

/// Errors produced while planning a command execution.
#[derive(Debug, Error)]
pub enum RunnerError {
    /// The command would have nothing to run.
    #[error("command must have at least one argument")]
    EmptyCommand,
    /// The timeout would immediately cancel the execution.
    #[error("timeout must be greater than zero")]
    ZeroTimeout,
}

/// A planned command execution, inspectable before it runs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandSpec {
    pub argv: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub timeout: Duration,
    pub open_terminal: bool,
}

impl CommandSpec {
    /// Creates a spec that runs `argv` with a sane default timeout.
    ///
    /// # Panics
    ///
    /// Panics if `argv` is empty; use [`CommandSpec::validate`] on untrusted
    /// input instead.
    #[must_use]
    pub fn new(argv: Vec<String>) -> Self {
        assert!(!argv.is_empty(), "argv must not be empty");
        Self {
            argv,
            cwd: None,
            env: BTreeMap::new(),
            timeout: Duration::from_secs(30),
            open_terminal: true,
        }
    }

    /// Sets a working directory.
    #[must_use]
    pub fn with_cwd(mut self, cwd: PathBuf) -> Self {
        self.cwd = Some(cwd);
        self
    }

    /// Sets the execution timeout.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Returns the exact command line shown to the user in the review screen.
    #[must_use]
    pub fn describe(&self) -> String {
        self.argv
            .iter()
            .map(|arg| describe_arg(arg))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Validates that the spec is safe to plan.
    ///
    /// # Errors
    ///
    /// Returns [`RunnerError::EmptyCommand`] when `argv` is empty and
    /// [`RunnerError::ZeroTimeout`] when the timeout is zero.
    pub fn validate(&self) -> Result<(), RunnerError> {
        if self.argv.is_empty() {
            return Err(RunnerError::EmptyCommand);
        }
        if self.timeout.is_zero() {
            return Err(RunnerError::ZeroTimeout);
        }
        Ok(())
    }
}

/// A cooperatively shared cancellation flag for in-flight executions.
#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    /// Creates a token that starts out uncancelled.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation. Safe to call from any thread, any number of
    /// times.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    /// Returns whether cancellation was requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

fn describe_arg(arg: &str) -> String {
    if arg.is_empty()
        || arg
            .chars()
            .any(|c| c.is_whitespace() || c == '"' || c == '\'' || c == '\\' || c == '$')
    {
        format!("{arg:?}")
    } else {
        arg.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describe_renders_the_exact_command_for_review() {
        let spec = CommandSpec::new(vec!["open".into(), "Herdr app.app".into(), "--wait".into()])
            .with_cwd(PathBuf::from("/tmp"));

        assert_eq!(spec.describe(), "open \"Herdr app.app\" --wait");
    }

    #[test]
    fn validation_rejects_empty_commands_and_zero_timeouts() {
        assert!(matches!(
            CommandSpec::new(vec!["echo".into()])
                .with_timeout(Duration::ZERO)
                .validate(),
            Err(RunnerError::ZeroTimeout)
        ));

        let mut empty = CommandSpec::new(vec!["echo".into()]);
        empty.argv.clear();
        assert!(matches!(empty.validate(), Err(RunnerError::EmptyCommand)));
    }

    #[test]
    fn cancellation_token_is_shared_and_observable() {
        let token = CancellationToken::new();
        let clone = token.clone();

        assert!(!token.is_cancelled());
        clone.cancel();
        assert!(token.is_cancelled());
    }
}
