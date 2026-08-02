//! Review-before-execute for imported confirmation-risk commands.
//!
//! The first execution of an imported confirmation-level action must display
//! the exact command and require approval (spec §15.2). [`ApprovalStore`]
//! turns a confirmation-risk command into a pending [`PendingReview`] that
//! must be approved before it may run; once approved, the same command line
//! stays approved so later presses are not re-prompted.

use std::collections::{BTreeSet, HashMap};

use thiserror::Error;

use crate::command::CommandSpec;
use crate::risk::RiskLevel;

/// A command waiting for a human to approve or deny it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingReview {
    /// Stable identifier a caller uses to approve or deny this review.
    pub id: String,
    /// The exact command under review.
    pub spec: CommandSpec,
}

/// What the store decided when a command was presented for execution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApprovalDecision {
    /// The command does not need approval (low risk or user-authored).
    NotRequired,
    /// The exact command was approved earlier and may run.
    AlreadyApproved,
    /// The command must be approved before it may run.
    Pending(PendingReview),
}

/// Errors produced while approving or denying a review.
#[derive(Debug, Error)]
pub enum ApprovalError {
    /// No pending review exists under this id.
    #[error("no pending review `{0}`")]
    UnknownReview(String),
}

/// Tracks pending and approved confirmation-risk commands.
#[derive(Debug, Default)]
pub struct ApprovalStore {
    next_id: u64,
    pending: HashMap<String, PendingReview>,
    approved: BTreeSet<String>,
}

impl ApprovalStore {
    /// Creates an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Presents `spec` for execution and decides whether it may run.
    ///
    /// Confirmation-risk commands that are not already approved become
    /// [`ApprovalDecision::Pending`] with the exact command line attached;
    /// everything else runs without review.
    #[must_use]
    pub fn request(&mut self, spec: &CommandSpec) -> ApprovalDecision {
        if spec.risk() != RiskLevel::Confirmation {
            return ApprovalDecision::NotRequired;
        }
        let fingerprint = spec.describe();
        if self.approved.contains(&fingerprint) {
            return ApprovalDecision::AlreadyApproved;
        }
        self.next_id += 1;
        let id = format!("review-{}", self.next_id);
        let review = PendingReview {
            id: id.clone(),
            spec: spec.clone(),
        };
        self.pending.insert(id.clone(), review.clone());
        ApprovalDecision::Pending(review)
    }

    /// Returns every pending review, in creation order.
    #[must_use]
    pub fn pending_reviews(&self) -> Vec<PendingReview> {
        let mut reviews: Vec<PendingReview> = self.pending.values().cloned().collect();
        reviews.sort_by(|a, b| a.id.cmp(&b.id));
        reviews
    }

    /// Approves a pending review, returning the exact command that may now run.
    ///
    /// # Errors
    ///
    /// Returns [`ApprovalError::UnknownReview`] when `id` is not pending.
    pub fn approve(&mut self, id: &str) -> Result<CommandSpec, ApprovalError> {
        let review = self
            .pending
            .remove(id)
            .ok_or_else(|| ApprovalError::UnknownReview(id.to_string()))?;
        self.approved.insert(review.spec.describe());
        Ok(review.spec)
    }

    /// Denies a pending review, dropping it without approving anything.
    ///
    /// # Errors
    ///
    /// Returns [`ApprovalError::UnknownReview`] when `id` is not pending.
    pub fn deny(&mut self, id: &str) -> Result<(), ApprovalError> {
        self.pending
            .remove(id)
            .map(|_| ())
            .ok_or_else(|| ApprovalError::UnknownReview(id.to_string()))
    }

    /// Returns whether this exact command line was already approved.
    #[must_use]
    pub fn is_approved(&self, spec: &CommandSpec) -> bool {
        self.approved.contains(&spec.describe())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(program: &str, args: &[&str]) -> CommandSpec {
        let mut full_argv = vec![program.to_string()];
        full_argv.extend(args.iter().map(|arg| (*arg).to_string()));
        CommandSpec::new(full_argv)
    }

    #[test]
    fn low_risk_commands_never_require_approval() {
        let mut store = ApprovalStore::new();

        let spec = command("echo", &["hi"]).with_imported(true);
        assert_eq!(store.request(&spec), ApprovalDecision::NotRequired);
    }

    #[test]
    fn confirmation_risk_commands_become_pending_with_the_exact_command() {
        let mut store = ApprovalStore::new();

        let spec = command("rm", &["-rf", "/tmp/x"]).with_imported(true);
        let decision = store.request(&spec);
        let ApprovalDecision::Pending(review) = decision else {
            panic!("imported destructive command must be pending");
        };
        assert_eq!(review.spec.describe(), "rm -rf /tmp/x");
        assert_eq!(store.pending_reviews().len(), 1);
    }

    #[test]
    fn approve_returns_the_command_and_records_the_fingerprint() {
        let mut store = ApprovalStore::new();

        let spec = command("rm", &["-rf", "/tmp/x"]).with_imported(true);
        let ApprovalDecision::Pending(review) = store.request(&spec) else {
            panic!("expected a pending review");
        };

        let approved = store.approve(&review.id).expect("approval succeeds");
        assert_eq!(approved, spec);
        assert!(store.is_approved(&spec));
        assert!(store.pending_reviews().is_empty());

        assert_eq!(store.request(&spec), ApprovalDecision::AlreadyApproved);
    }

    #[test]
    fn deny_drops_the_review_without_approving() {
        let mut store = ApprovalStore::new();

        let spec = command("rm", &["-rf", "/tmp/x"]).with_imported(true);
        let ApprovalDecision::Pending(review) = store.request(&spec) else {
            panic!("expected a pending review");
        };

        store.deny(&review.id).expect("denial succeeds");
        assert!(!store.is_approved(&spec));
        assert!(store.pending_reviews().is_empty());
        // The same command asks again instead of running.
        assert!(matches!(store.request(&spec), ApprovalDecision::Pending(_)));
    }

    #[test]
    fn approving_or_denying_an_unknown_review_fails() {
        let mut store = ApprovalStore::new();

        assert!(matches!(
            store.approve("review-999"),
            Err(ApprovalError::UnknownReview(_))
        ));
        assert!(matches!(
            store.deny("review-999"),
            Err(ApprovalError::UnknownReview(_))
        ));
    }
}
