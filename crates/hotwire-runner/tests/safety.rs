//! End-to-end safety flows (SAFE-001 acceptance): risk classification,
//! timeout/cancellation, environment redaction, imported-command approval, and
//! the review-before-execute boundary.

use std::time::Duration;

use hotwire_runner::{
    ApprovalDecision, ApprovalStore, CancellationToken, CommandRunner, CommandSpec, CwdStrategy,
    LogCategory, RunStatus, SafetyLog,
};

fn imported(program: &str, args: &[&str]) -> CommandSpec {
    let full_argv = std::iter::once(program.to_string())
        .chain(args.iter().map(|arg| (*arg).to_string()))
        .collect();
    CommandSpec::new(full_argv)
        .with_imported(true)
        .with_open_terminal(false)
}

#[tokio::test]
async fn imported_confirmation_commands_must_be_approved_before_they_run() {
    let mut store = ApprovalStore::new();
    let runner = CommandRunner::new();
    let token = CancellationToken::new();

    // The first execution is gated behind an explicit review of the exact
    // command line.
    let spec = imported("rm", &["-rf", "/tmp/nonexistent"]);
    let ApprovalDecision::Pending(review) = store.request(&spec) else {
        panic!("an imported destructive command must become a pending review");
    };
    assert_eq!(review.spec.describe(), "rm -rf /tmp/nonexistent");
    assert!(!store.is_approved(&spec));

    // Approval returns the exact command, and later presses are not re-prompted.
    let approved = store.approve(&review.id).expect("approval succeeds");
    assert_eq!(approved, spec);
    assert_eq!(store.request(&spec), ApprovalDecision::AlreadyApproved);

    let output = runner.run(&spec, &token, None).await;
    assert_eq!(output.status, RunStatus::Succeeded { exit_code: 0 });
}

#[tokio::test]
async fn timeout_kills_runaway_commands_and_cancellation_stops_them() {
    let runner = CommandRunner::new();

    let timeout_token = CancellationToken::new();
    let timeout_spec =
        imported("/bin/sh", &["-c", "sleep 30"]).with_timeout(Duration::from_millis(150));
    let start = std::time::Instant::now();
    let output = runner.run(&timeout_spec, &timeout_token, None).await;
    assert_eq!(output.status, RunStatus::TimedOut);
    assert!(start.elapsed() < Duration::from_secs(5));

    let cancel_token = CancellationToken::new();
    let cancel_spec = imported("/bin/sh", &["-c", "sleep 30"]);
    let task = tokio::spawn({
        let runner = runner.clone();
        let cancel_token = cancel_token.clone();
        let cancel_spec = cancel_spec.clone();
        async move { runner.run(&cancel_spec, &cancel_token, None).await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    cancel_token.cancel();
    let output = task.await.expect("runner task joins");
    assert_eq!(output.status, RunStatus::Cancelled);
}

#[tokio::test]
async fn environment_values_are_sanitized_and_secrets_redacted() {
    // A sanitized env rebuilds the child environment from an allowlist; the
    // child never sees the host env wholesale.
    let runner = CommandRunner::new();
    let token = CancellationToken::new();
    let mut env = hotwire_runner::SanitizedEnv::new()
        .with_var("HOTWIRE_MODE", "safe")
        .with_var("API_TOKEN", "super-secret-value");
    env.mark_secret("API_TOKEN");

    let probe = CommandSpec::new(vec![
        "/bin/sh".into(),
        "-c".into(),
        "printf '%s' \"$HOTWIRE_MODE\" \"$API_TOKEN\" \"$UNINHERITED\"".into(),
    ])
    .with_cwd(CwdStrategy::Home)
    .with_env(env)
    .with_open_terminal(false);

    let output = runner.run(&probe, &token, None).await;
    assert_eq!(output.stdout, "safesuper-secret-value");

    // The redacted surface never reveals the secret value.
    let mut log = SafetyLog::memory();
    log.add_redactor_literal("super-secret-value");
    log.info(
        LogCategory::Execution,
        format!("ran probe with {}", output.stdout),
    )
    .expect("log writes");
    assert!(!log.sink().joined().contains("super-secret-value"));
}

#[test]
fn development_commands_default_to_a_visible_terminal() {
    let spec = CommandSpec::new(vec!["make".into(), "test".into()]);
    assert!(spec.open_terminal);
}
