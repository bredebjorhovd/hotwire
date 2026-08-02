//! Review-before-execute for imported confirmation-risk commands.
//!
//! The first execution of an imported confirmation-level action must display
//! the exact command and require approval (spec §15.2). [`ApprovalStore`]
//! turns a confirmation-risk [`ResolvedPlan`] into a pending [`PendingReview`]
//! that must be approved before it may run. An approval is bound to the
//! *complete resolved execution plan* — argv, the exact working directory, the
//! full resolved environment snapshot, timeout, terminal mode, and provenance —
//! so changing the current project or an inherited environment value forces a
//! new review. The displayed command stays human-readable via
//! [`ResolvedPlan::describe`].

use std::collections::{BTreeSet, HashMap};

use thiserror::Error;

use crate::command::ResolvedPlan;
use crate::id::ReviewId;
use crate::risk::RiskLevel;

/// A command waiting for a human to approve or deny it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingReview {
    /// Stable, internally generated identifier a caller uses to approve or
    /// deny this review.
    pub id: ReviewId,
    /// The exact resolved plan under review (the displayed argv is
    /// [`ResolvedPlan::describe`]).
    pub plan: ResolvedPlan,
}

/// What the store decided when a plan was presented for execution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApprovalDecision {
    /// The command does not need approval (low risk or user-authored).
    NotRequired,
    /// This exact resolved plan was approved earlier and may run.
    AlreadyApproved,
    /// The plan must be approved before it may run.
    Pending(PendingReview),
}

/// Errors produced while approving or denying a review.
#[derive(Debug, Error)]
pub enum ApprovalError {
    /// No pending review exists under this id.
    #[error("no pending review `{0}`")]
    UnknownReview(String),
}

/// Tracks pending and approved confirmation-risk execution plans.
///
/// Approved plans are stored by full structural equality, so approval is bound
/// to the complete immutable resolved plan — not to a rendered string and not
/// to the unresolved spec.
#[derive(Debug, Default)]
pub struct ApprovalStore {
    next_id: u64,
    pending: HashMap<String, PendingReview>,
    approved: BTreeSet<ResolvedPlan>,
}

impl ApprovalStore {
    /// Creates an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Presents `plan` for execution and decides whether it may run.
    ///
    /// Confirmation-risk plans that are not already approved become
    /// [`ApprovalDecision::Pending`] with the exact command line attached;
    /// everything else runs without review. Presenting an already-pending plan
    /// returns the same review instead of piling up duplicates.
    ///
    /// # Panics
    ///
    /// Panics if the internally generated review id fails its own validation
    /// (impossible by construction; the store only ever emits `review-<n>`).
    #[must_use]
    pub fn request(&mut self, plan: &ResolvedPlan) -> ApprovalDecision {
        if plan.risk() != RiskLevel::Confirmation {
            return ApprovalDecision::NotRequired;
        }
        if self.approved.contains(plan) {
            return ApprovalDecision::AlreadyApproved;
        }
        if let Some(existing) = self.pending.values().find(|review| review.plan == *plan) {
            return ApprovalDecision::Pending(existing.clone());
        }
        self.next_id += 1;
        let id = ReviewId::try_new(format!("review-{}", self.next_id))
            .expect("internally generated review ids are always valid");
        let review = PendingReview {
            id: id.clone(),
            plan: plan.clone(),
        };
        self.pending.insert(id.as_str().to_string(), review.clone());
        ApprovalDecision::Pending(review)
    }

    /// Returns every pending review, in creation order.
    #[must_use]
    pub fn pending_reviews(&self) -> Vec<PendingReview> {
        let mut reviews: Vec<PendingReview> = self.pending.values().cloned().collect();
        reviews.sort_by(|a, b| a.id.cmp(&b.id));
        reviews
    }

    /// Approves a pending review, returning the exact plan that may now run.
    ///
    /// # Errors
    ///
    /// Returns [`ApprovalError::UnknownReview`] when `id` is not pending.
    pub fn approve(&mut self, id: &str) -> Result<ResolvedPlan, ApprovalError> {
        let review = self
            .pending
            .remove(id)
            .ok_or_else(|| ApprovalError::UnknownReview(id.to_string()))?;
        self.approved.insert(review.plan.clone());
        Ok(review.plan)
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

    /// Returns whether this exact resolved plan was already approved.
    #[must_use]
    pub fn is_approved(&self, plan: &ResolvedPlan) -> bool {
        self.approved.contains(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(program: &str, args: &[&str], imported: bool) -> ResolvedPlan {
        let mut full_argv = vec![program.to_string()];
        full_argv.extend(args.iter().map(|arg| (*arg).to_string()));
        let spec = crate::command::CommandSpec::new(full_argv)
            .with_imported(imported)
            .with_open_terminal(false);
        spec.resolve(None).expect("plan resolves")
    }

    #[test]
    fn low_risk_plans_never_require_approval() {
        let mut store = ApprovalStore::new();

        let plan = plan("echo", &["hi"], true);
        assert_eq!(store.request(&plan), ApprovalDecision::NotRequired);
    }

    #[test]
    fn confirmation_risk_plans_become_pending_with_the_exact_command() {
        let mut store = ApprovalStore::new();

        let plan = plan("rm", &["-rf", "/tmp/x"], true);
        let decision = store.request(&plan);
        let ApprovalDecision::Pending(review) = decision else {
            panic!("imported destructive command must be pending");
        };
        assert_eq!(review.plan.describe(), "rm -rf /tmp/x");
        assert_eq!(store.pending_reviews().len(), 1);
    }

    #[test]
    fn approve_returns_the_plan_and_records_the_exact_spec() {
        let mut store = ApprovalStore::new();

        let plan = plan("rm", &["-rf", "/tmp/x"], true);
        let ApprovalDecision::Pending(review) = store.request(&plan) else {
            panic!("expected a pending review");
        };

        let approved = store
            .approve(review.id.as_str())
            .expect("approval succeeds");
        assert_eq!(approved, plan);
        assert!(store.is_approved(&plan));
        assert!(store.pending_reviews().is_empty());

        assert_eq!(store.request(&plan), ApprovalDecision::AlreadyApproved);
    }

    #[test]
    fn deny_drops_the_review_without_approving() {
        let mut store = ApprovalStore::new();

        let plan = plan("rm", &["-rf", "/tmp/x"], true);
        let ApprovalDecision::Pending(review) = store.request(&plan) else {
            panic!("expected a pending review");
        };

        store.deny(review.id.as_str()).expect("denial succeeds");
        assert!(!store.is_approved(&plan));
        assert!(store.pending_reviews().is_empty());
        // The same plan asks again instead of running.
        assert!(matches!(store.request(&plan), ApprovalDecision::Pending(_)));
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

    #[test]
    fn approval_is_bound_to_the_full_resolved_plan() {
        let mut store = ApprovalStore::new();
        let base = plan("rm", &["-rf", "/tmp/x"], true);
        let ApprovalDecision::Pending(review) = store.request(&base) else {
            panic!("expected a pending review");
        };
        store
            .approve(review.id.as_str())
            .expect("approval succeeds");

        // The identical plan is approved; any mutation is a different plan and
        // must be reviewed again.
        assert!(store.is_approved(&base));
        assert_eq!(store.request(&base), ApprovalDecision::AlreadyApproved);

        let mut changed_argv = base.clone();
        changed_argv.argv[2] = "/tmp/y".to_string();
        let mut changed_cwd = base.clone();
        changed_cwd.cwd = "/elsewhere".into();
        let mut changed_env = base.clone();
        changed_env.env.insert("EXTRA".to_string(), "1".to_string());
        let mut changed_timeout = base.clone();
        changed_timeout.timeout = std::time::Duration::from_secs(1);
        let mut changed_terminal = base.clone();
        changed_terminal.open_terminal = true;

        for mutated in [
            changed_argv,
            changed_cwd,
            changed_env,
            changed_timeout,
            changed_terminal,
        ] {
            assert!(
                !store.is_approved(&mutated),
                "a mutated plan must not reuse an approval"
            );
            assert!(
                matches!(store.request(&mutated), ApprovalDecision::Pending(_)),
                "a mutated plan must be reviewed again"
            );
        }
    }

    #[test]
    fn approval_does_not_survive_a_changed_inherited_environment() {
        let mut store = ApprovalStore::new();
        let spec = crate::command::CommandSpec::new(vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "printf ok".to_string(),
        ])
        .with_imported(true)
        .with_open_terminal(false)
        .with_env(crate::env::SanitizedEnv::new().inherit("PATH"));

        let mut host_a = std::collections::BTreeMap::new();
        host_a.insert("PATH".to_string(), "/usr/bin".to_string());
        let mut host_b = std::collections::BTreeMap::new();
        host_b.insert("PATH".to_string(), "/custom/bin".to_string());

        let plan_a = spec.resolve_with(None, &host_a).expect("resolves");
        let plan_b = spec.resolve_with(None, &host_b).expect("resolves");
        assert_ne!(
            plan_a, plan_b,
            "a different inherited PATH is a different plan"
        );

        let ApprovalDecision::Pending(review) = store.request(&plan_a) else {
            panic!("imported sh must be confirmation-risk");
        };
        store
            .approve(review.id.as_str())
            .expect("approval succeeds");
        assert!(store.is_approved(&plan_a));
        assert!(
            !store.is_approved(&plan_b),
            "an approval must not survive a changed inherited value"
        );
        assert!(matches!(
            store.request(&plan_b),
            ApprovalDecision::Pending(_)
        ));
    }

    #[test]
    fn approval_tracks_provenance_of_imported_scripts() {
        let mut store = ApprovalStore::new();
        let imported_plan = plan("./deploy.sh", &[], true);
        let ApprovalDecision::Pending(review) = store.request(&imported_plan) else {
            panic!("an arbitrary imported script must be confirmation-risk");
        };
        store
            .approve(review.id.as_str())
            .expect("approval succeeds");
        assert!(store.is_approved(&imported_plan));

        // The same script authored by the user is low risk: it needs no review
        // and does not reuse the imported approval.
        let trusted = plan("./deploy.sh", &[], false);
        assert!(!store.is_approved(&trusted));
        assert_eq!(store.request(&trusted), ApprovalDecision::NotRequired);
    }

    #[test]
    fn presenting_a_pending_plan_returns_the_same_review() {
        let mut store = ApprovalStore::new();
        let plan = plan("rm", &["-rf", "/tmp/x"], true);

        let ApprovalDecision::Pending(first) = store.request(&plan) else {
            panic!("expected a pending review");
        };
        let ApprovalDecision::Pending(second) = store.request(&plan) else {
            panic!("expected the same pending review");
        };
        assert_eq!(first.id, second.id);
        assert_eq!(store.pending_reviews().len(), 1);
    }
}
