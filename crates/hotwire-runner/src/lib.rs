//! Safe command execution boundary.
//!
//! The runner turns shell and script action configs into inspectable,
//! cancellable executions with timeouts. It owns the *review-before-execute*
//! boundary (spec §15.2): imported confirmation-risk commands expose their
//! exact command line and require explicit approval before the first run. It
//! also owns the *redacted local log* boundary (spec §15.1/§15.3): log entries
//! carry only a closed set of diagnostic fields, and every message passes
//! through a [`Redactor`] so typed text, prompts, secrets, and arbitrary key
//! sequences never reach a log.
//!
//! Commands are expressed as argument arrays ([`CommandSpec::argv`]), never as
//! shell strings, and run under a sanitized environment: the host environment
//! is cleared and rebuilt from an explicit allowlist plus explicitly declared
//! variables ([`SanitizedEnv`]). Working directories come from a declared
//! [`CwdStrategy`], not from ambient state. The default for user-authored
//! development commands is a visible terminal (spec §13.3).
//!
//! Execution never runs on the native input callback thread; callers drive
//! [`CommandRunner`] from an async task.

mod command;
mod env;
mod log;
mod redact;
mod review;
mod risk;
mod run;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub use command::{CommandError, CommandSpec, CwdStrategy, ResolvedCwd};
pub use env::{SanitizedEnv, SecretSet};
pub use log::{FileSink, InMemorySink, LogCategory, LogEntry, LogLevel, LogSink, SafetyLog};
pub use redact::{Redactor, REDACTED};
pub use review::{ApprovalDecision, ApprovalError, ApprovalStore, PendingReview};
pub use risk::{classify_command_risk, RiskLevel};
pub use run::{CommandOutput, CommandRunner, RunStatus};

/// A cooperatively shared cancellation flag for in-flight executions.
///
/// Cheap to clone and share across tasks. Safe to call from any thread; the
/// token may be awaited for cancellation from an async context. Backed by a
/// `watch` channel with a keep-alive receiver, so a cancellation that lands
/// between subscribing and awaiting is never lost and any number of tasks can
/// wait.
#[derive(Clone, Debug)]
pub struct CancellationToken {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    cancelled: AtomicBool,
    sender: tokio::sync::watch::Sender<bool>,
    /// Keeps the channel open so `send` always stores the value, even when no
    /// task is waiting yet. Held but never read.
    #[allow(dead_code)]
    keep_alive: tokio::sync::watch::Receiver<bool>,
}

impl CancellationToken {
    /// Creates a token that starts out uncancelled.
    #[must_use]
    pub fn new() -> Self {
        let (sender, keep_alive) = tokio::sync::watch::channel(false);
        Self {
            inner: Arc::new(Inner {
                cancelled: AtomicBool::new(false),
                sender,
                keep_alive,
            }),
        }
    }

    /// Requests cancellation. Safe to call from any thread, any number of
    /// times; awaiting tasks wake on the next [`CancellationToken::cancelled`].
    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::Relaxed);
        let _ = self.inner.sender.send(true);
    }

    /// Returns whether cancellation was requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Relaxed)
    }

    /// Awaits cancellation, returning immediately if already cancelled.
    ///
    /// This is the async counterpart to [`CancellationToken::is_cancelled`];
    /// the runner uses it to terminate a child process the moment the token is
    /// cancelled.
    pub async fn cancelled(&self) {
        let mut receiver = self.inner.sender.subscribe();
        if *receiver.borrow() {
            return;
        }
        let _ = receiver.changed().await;
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancelling_before_waiting_wakes_immediately() {
        let token = CancellationToken::new();
        token.cancel();
        token.cancelled().await;
    }

    #[tokio::test]
    async fn cancelling_while_waiting_wakes_the_waiter() {
        let token = CancellationToken::new();
        let awaited = {
            let token = token.clone();
            tokio::spawn(async move { token.cancelled().await })
        };
        token.cancel();
        awaited.await.expect("cancelled future completes");
    }

    #[tokio::test]
    async fn cancellation_wakes_every_waiting_task() {
        let token = CancellationToken::new();
        let tasks: Vec<_> = (0..3)
            .map(|_| {
                let token = token.clone();
                tokio::spawn(async move { token.cancelled().await })
            })
            .collect();
        token.cancel();
        for task in tasks {
            task.await.expect("every waiter wakes");
        }
    }
}
