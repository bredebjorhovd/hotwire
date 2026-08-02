//! End-to-end safety flows (SAFE-001 acceptance): risk classification,
//! timeout/cancellation, environment redaction, imported-command approval, and
//! the review-before-execute boundary enforced by the runner on the resolved
//! plan.

use std::time::Duration;

use hotwire_runner::{
    ApprovalDecision, CancellationToken, CommandRunner, CommandSpec, CwdStrategy, EventDetail,
    LogCategory, RunStatus, SafetyLog,
};

fn build_command(program: &str, args: &[&str]) -> CommandSpec {
    let full_argv = std::iter::once(program.to_string())
        .chain(args.iter().map(|arg| (*arg).to_string()))
        .collect();
    CommandSpec::new(full_argv)
}

/// A harmless imported command that is still confirmation-risk (`sh` is not an
/// approved CLI, so an imported `sh` needs review). Tests never execute a
/// destructive program.
fn imported_harmless() -> CommandSpec {
    build_command("/bin/sh", &["-c", "printf ok"])
        .with_imported(true)
        .with_open_terminal(false)
}

fn background(program: &str, args: &[&str]) -> CommandSpec {
    build_command(program, args).with_open_terminal(false)
}

#[tokio::test]
async fn imported_confirmation_commands_require_approval_before_they_run() {
    let runner = CommandRunner::new();
    let token = CancellationToken::new();
    let spec = imported_harmless();
    let plan = spec.resolve(None).expect("plan resolves");

    // The review surface shows the exact command line before anything runs.
    let ApprovalDecision::Pending(review) = runner.request_approval(&plan) else {
        panic!("an imported confirmation command must become a pending review");
    };
    assert_eq!(review.plan.describe(), "/bin/sh -c \"printf ok\"");

    // The direct run is refused until the exact resolved plan is approved.
    let refused = runner.run(&spec, &token, None).await;
    assert!(
        matches!(refused.status, RunStatus::ApprovalRequired(_)),
        "a direct imported confirmation run must not start before approval"
    );

    let approved = runner
        .approve(review.id.as_str())
        .expect("approval succeeds");
    assert_eq!(approved, plan);
    assert_eq!(
        runner.request_approval(&plan),
        ApprovalDecision::AlreadyApproved
    );

    let output = runner.run(&spec, &token, None).await;
    assert_eq!(output.status, RunStatus::Succeeded { exit_code: 0 });
    assert_eq!(output.stdout, "ok");
}

#[tokio::test]
async fn timeout_kills_runaway_commands_and_cancellation_stops_them() {
    let runner = CommandRunner::new();

    let timeout_token = CancellationToken::new();
    let timeout_spec = background("/bin/sleep", &["30"]).with_timeout(Duration::from_millis(150));
    let start = std::time::Instant::now();
    let output = runner.run(&timeout_spec, &timeout_token, None).await;
    assert_eq!(output.status, RunStatus::TimedOut);
    assert!(start.elapsed() < Duration::from_secs(5));

    let cancel_token = CancellationToken::new();
    let cancel_spec = background("/bin/sleep", &["30"]);
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

    let probe = CommandSpec::new(vec!["/usr/bin/printenv".into(), "API_TOKEN".into()])
        .with_cwd(CwdStrategy::Home)
        .with_env(env.clone())
        .with_open_terminal(false);

    let output = runner.run(&probe, &token, None).await;
    assert_eq!(output.status, RunStatus::Succeeded { exit_code: 0 });
    assert_eq!(output.stdout.trim_end(), "super-secret-value");

    // A redactor seeded from the resolved environment masks the secret value
    // even when the child repeats it bare into its output.
    let redactor = env.redactor();
    let masked = redactor.redact(&output.stdout);
    assert!(!masked.contains("super-secret-value"));
    assert!(masked.contains("[REDACTED]"));

    // A non-secret explicit variable also reaches the child.
    let mode = CommandSpec::new(vec!["/usr/bin/printenv".into(), "HOTWIRE_MODE".into()])
        .with_cwd(CwdStrategy::Home)
        .with_env(env)
        .with_open_terminal(false);
    let mode_output = runner.run(&mode, &token, None).await;
    assert_eq!(mode_output.stdout.trim_end(), "safe");

    // The persistent log carries only structured details — no free text.
    let mut log = SafetyLog::memory();
    log.info(LogCategory::Execution, EventDetail::ExecutionSucceeded)
        .expect("log writes");
    assert_eq!(log.sink().details(), vec![&EventDetail::ExecutionSucceeded]);
}

#[test]
fn development_commands_default_to_a_visible_terminal() {
    let spec = CommandSpec::new(vec!["make".into(), "test".into()]);
    assert!(spec.open_terminal);
}
