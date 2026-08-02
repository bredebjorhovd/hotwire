//! Process execution with timeouts and cancellation.
//!
//! [`CommandRunner`] spawns a [`CommandSpec`] as an argument array with a
//! sanitized environment, a resolved working directory, a hard timeout, and a
//! shared [`CancellationToken`](crate::CancellationToken). It is the *only*
//! public execution path, and it enforces review-before-execute: an imported
//! confirmation-risk command cannot start until its exact structured spec has
//! been approved (spec §15.2).
//!
//! Background commands run in their own process group, so a timeout or
//! cancellation terminates the whole group — the command and any descendants —
//! not just the immediate child. A visible-terminal command (the default for
//! development commands, spec §13.3) is handed to a real terminal session and
//! is explicitly *untracked*: it claims no timeout or cancellation coverage.
//! Spawning and waiting always happen on a tokio task — never on a native
//! input callback thread.

use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncRead, AsyncReadExt};

use crate::command::{CommandError, CommandSpec, ResolvedCwd};
use crate::review::{ApprovalDecision, ApprovalError, ApprovalStore, PendingReview};
use crate::CancellationToken;

/// Maximum bytes of stdout/stderr captured from a background command.
const OUTPUT_CAP: usize = 64 * 1024;

/// How a tracked command ended.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RunStatus {
    /// The process exited with this (non-negative) code.
    Succeeded { exit_code: i32 },
    /// The process exited with a failure code.
    Failed { exit_code: i32 },
    /// The command was cancelled before finishing; its process group was killed.
    Cancelled,
    /// The command exceeded its timeout; its process group was killed.
    TimedOut,
    /// The command was handed to a visible terminal. **Untracked**: the runner
    /// does not wait on it and offers no timeout or cancellation coverage.
    SpawnedInTerminal,
    /// An imported confirmation-risk command must be approved before it can
    /// run. The payload is the id of the pending review to approve.
    ApprovalRequired(String),
    /// The command could not be started.
    StartError(String),
}

/// The result of one tracked execution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandOutput {
    /// How the command ended.
    pub status: RunStatus,
    /// Captured stdout (capped), empty for visible-terminal commands.
    pub stdout: String,
    /// Captured stderr (capped), empty for visible-terminal commands.
    pub stderr: String,
}

/// The single public command execution path.
///
/// Owns the approval store, so review-before-execute cannot be bypassed: a
/// confirmation-risk imported command is refused with
/// [`RunStatus::ApprovalRequired`] until its exact spec is approved through
/// this runner. Cheap to clone and share.
#[derive(Clone, Debug, Default)]
pub struct CommandRunner {
    approvals: Arc<Mutex<ApprovalStore>>,
}

fn lock(store: &Mutex<ApprovalStore>) -> std::sync::MutexGuard<'_, ApprovalStore> {
    store
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl CommandRunner {
    /// Creates a runner with an empty approval store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Presents `spec` for review, returning the approval decision.
    ///
    /// This is how a caller shows the review screen *before* the first run:
    /// the decision carries the pending review (with the exact command) to
    /// display.
    #[must_use]
    pub fn request_approval(&self, spec: &CommandSpec) -> ApprovalDecision {
        lock(&self.approvals).request(spec)
    }

    /// Approves a pending review, returning the exact command that may now run.
    ///
    /// # Errors
    ///
    /// Returns [`ApprovalError::UnknownReview`] when `review_id` is not pending.
    pub fn approve(&self, review_id: &str) -> Result<CommandSpec, ApprovalError> {
        lock(&self.approvals).approve(review_id)
    }

    /// Denies a pending review, dropping it without approving anything.
    ///
    /// # Errors
    ///
    /// Returns [`ApprovalError::UnknownReview`] when `review_id` is not pending.
    pub fn deny(&self, review_id: &str) -> Result<(), ApprovalError> {
        lock(&self.approvals).deny(review_id)
    }

    /// Returns every pending review, in creation order.
    #[must_use]
    pub fn pending_reviews(&self) -> Vec<PendingReview> {
        lock(&self.approvals).pending_reviews()
    }

    /// Returns whether this exact structured spec was already approved.
    #[must_use]
    pub fn is_approved(&self, spec: &CommandSpec) -> bool {
        lock(&self.approvals).is_approved(spec)
    }

    /// Runs `spec`, cancelling the child's process group when `token` is
    /// cancelled or the spec timeout elapses.
    ///
    /// `project` supplies the current-project hint used by the
    /// [`CwdStrategy::CurrentProject`](crate::CwdStrategy) strategy.
    ///
    /// An imported confirmation-risk command that has not been approved
    /// returns [`RunStatus::ApprovalRequired`] without starting anything. A
    /// visible-terminal command is handed to a real terminal and returns
    /// [`RunStatus::SpawnedInTerminal`] without tracking. Any other background
    /// command is tracked to completion, timeout, or cancellation.
    pub async fn run(
        &self,
        spec: &CommandSpec,
        token: &CancellationToken,
        project: Option<&Path>,
    ) -> CommandOutput {
        if let Err(error) = spec.validate() {
            return CommandOutput {
                status: RunStatus::StartError(error.to_string()),
                stdout: String::new(),
                stderr: String::new(),
            };
        }

        // Review-before-execute is enforced here, on the only execution path.
        match self.request_approval(spec) {
            ApprovalDecision::NotRequired | ApprovalDecision::AlreadyApproved => {}
            ApprovalDecision::Pending(review) => {
                return CommandOutput {
                    status: RunStatus::ApprovalRequired(review.id),
                    stdout: String::new(),
                    stderr: String::new(),
                };
            }
        }

        let cwd = match spec.resolve_cwd(project) {
            Ok(ResolvedCwd::Working(dir)) => Some(dir),
            Ok(ResolvedCwd::AskUser) => {
                return CommandOutput {
                    status: RunStatus::StartError(CommandError::CwdUnresolved.to_string()),
                    stdout: String::new(),
                    stderr: String::new(),
                };
            }
            Err(error) => {
                return CommandOutput {
                    status: RunStatus::StartError(error.to_string()),
                    stdout: String::new(),
                    stderr: String::new(),
                };
            }
        };

        if spec.open_terminal {
            return match spawn_visible_terminal(spec, cwd.as_deref()).await {
                Ok(()) => CommandOutput {
                    status: RunStatus::SpawnedInTerminal,
                    stdout: String::new(),
                    stderr: String::new(),
                },
                Err(error) => CommandOutput {
                    status: RunStatus::StartError(error),
                    stdout: String::new(),
                    stderr: String::new(),
                },
            };
        }

        run_background(spec, token, cwd.as_deref()).await
    }
}

/// Builds the POSIX script handed to Terminal.app for a visible-terminal run.
///
/// The script `cd`s into the fixed working directory (when one was resolved),
/// then `exec env -i` with only the resolved sanitized environment followed by
/// the argument array. Every word is single-quoted with [`sh_quote`], so
/// spaces, quotes, `$VAR`, `$()`, and newlines inside an argument are literal
/// and cannot expand or inject. The host environment is never passed through.
#[cfg(target_os = "macos")]
#[must_use]
fn build_terminal_script(spec: &CommandSpec, cwd: Option<&Path>) -> String {
    let mut script = String::new();
    if let Some(dir) = cwd {
        script.push_str("cd ");
        script.push_str(&sh_quote(&dir.to_string_lossy()));
        script.push_str("; ");
    }
    script.push_str("exec env -i");
    for (key, value) in spec.env.build() {
        script.push(' ');
        script.push_str(&sh_quote(&format!("{key}={value}")));
    }
    for arg in &spec.argv {
        script.push(' ');
        script.push_str(&sh_quote(arg));
    }
    script
}

/// Quotes a string as a single POSIX shell word (single-quote escaping).
///
/// Everything between the single quotes is literal to the shell, including
/// `$`, backticks, backslashes, and newlines; an embedded `'` becomes
/// `'\''`.
#[cfg(target_os = "macos")]
#[must_use]
fn sh_quote(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 2);
    out.push('\'');
    for c in input.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// Encodes a string as an `AppleScript` string literal (double quotes).
///
/// `AppleScript` strings are double-quoted and escape `"` and `\`; this is the
/// correct encoding for the `osascript -e` argument (the whole `AppleScript`
/// is passed as a single argv element, so no shell quoting is involved).
#[cfg(target_os = "macos")]
#[must_use]
fn applescript_quote(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 2);
    out.push('"');
    for c in input.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Hands the command to a visible terminal on macOS.
///
/// Runs `osascript` to make Terminal.app execute the quoted script, then waits
/// for osascript to finish so a quoting or launch failure surfaces here as an
/// error instead of a false "spawned" report. The command itself is untracked:
/// the runner does not wait on it, and no timeout or cancellation applies.
#[cfg(target_os = "macos")]
async fn spawn_visible_terminal(spec: &CommandSpec, cwd: Option<&Path>) -> Result<(), String> {
    let script = build_terminal_script(spec, cwd);
    let apple = format!(
        "tell application \"Terminal\" to do script {}",
        applescript_quote(&script)
    );
    let mut child = tokio::process::Command::new("osascript")
        .arg("-e")
        .arg(&apple)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("failed to launch osascript: {error}"))?;
    let status = child
        .wait()
        .await
        .map_err(|error| format!("failed to wait for the terminal handoff: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "the terminal handoff failed (osascript exit {status})"
        ))
    }
}

/// Hands the command to the shell directly where no terminal wrapper exists.
///
/// The fallback spawns the command untracked with the sanitized environment;
/// like the macOS path it claims no timeout or cancellation coverage.
#[cfg(not(target_os = "macos"))]
async fn spawn_visible_terminal(spec: &CommandSpec, cwd: Option<&Path>) -> Result<(), String> {
    let mut command = tokio::process::Command::new(&spec.argv[0]);
    command.args(&spec.argv[1..]);
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    command.env_clear();
    for (key, value) in spec.env.build() {
        command.env(key, value);
    }
    match command.spawn() {
        Ok(_) => Ok(()),
        Err(error) => Err(format!("failed to spawn: {error}")),
    }
}

/// Puts the child into its own process group so its whole tree can be killed.
#[cfg(unix)]
fn configure_process_group(command: &mut tokio::process::Command) {
    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut tokio::process::Command) {}

/// Terminates a background command: the whole process group, then the direct
/// child, then reaps it.
async fn terminate(child: &mut tokio::process::Child) {
    kill_process_group(child);
    let _ = child.kill().await;
    let _ = child.wait().await;
}

/// SIGKILLs the process group whose id equals the child's pid.
#[cfg(unix)]
fn kill_process_group(child: &tokio::process::Child) {
    if let Some(pid) = child.id() {
        if let Ok(pgid) = i32::try_from(pid) {
            let _ = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(-pgid),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
    }
}

#[cfg(not(unix))]
fn kill_process_group(_child: &tokio::process::Child) {}

/// Tracks a background command to completion, timeout, or cancellation.
async fn run_background(
    spec: &CommandSpec,
    token: &CancellationToken,
    cwd: Option<&Path>,
) -> CommandOutput {
    let mut command = tokio::process::Command::new(&spec.argv[0]);
    command.args(&spec.argv[1..]);
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    // Sanitized environment: clear the host env and rebuild from the spec.
    command.env_clear();
    for (key, value) in spec.env.build() {
        command.env(key, value);
    }
    command.kill_on_drop(true);
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    configure_process_group(&mut command);

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return CommandOutput {
                status: RunStatus::StartError(format!(
                    "failed to spawn `{}`: {error}",
                    spec.argv[0]
                )),
                stdout: String::new(),
                stderr: String::new(),
            };
        }
    };

    let stdout = tokio::spawn(read_capped(child.stdout.take(), OUTPUT_CAP));
    let stderr = tokio::spawn(read_capped(child.stderr.take(), OUTPUT_CAP));

    let timeout = tokio::time::sleep(spec.timeout);
    tokio::pin!(timeout);

    tokio::select! {
        result = child.wait() => {
            let exit_code = result.map_or(-1, |status| status.code().unwrap_or(-1));
            let stdout = stdout.await.unwrap_or_default();
            let stderr = stderr.await.unwrap_or_default();
            let status = if exit_code == 0 {
                RunStatus::Succeeded { exit_code }
            } else {
                RunStatus::Failed { exit_code }
            };
            CommandOutput { status, stdout, stderr }
        }
        () = token.cancelled() => {
            terminate(&mut child).await;
            let _ = stdout.await;
            let _ = stderr.await;
            CommandOutput {
                status: RunStatus::Cancelled,
                stdout: String::new(),
                stderr: String::new(),
            }
        }
        () = &mut timeout => {
            terminate(&mut child).await;
            let _ = stdout.await;
            let _ = stderr.await;
            CommandOutput {
                status: RunStatus::TimedOut,
                stdout: String::new(),
                stderr: String::new(),
            }
        }
    }
}

/// Reads a piped stream up to `cap` bytes, returning the lossy text.
async fn read_capped<R: AsyncRead + Unpin>(reader: Option<R>, cap: usize) -> String {
    let Some(mut reader) = reader else {
        return String::new();
    };
    let mut bytes = Vec::with_capacity(cap.min(8192));
    let mut chunk = [0u8; 8192];
    loop {
        let read = reader.read(&mut chunk).await.unwrap_or(0);
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
        if bytes.len() >= cap {
            break;
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn background(program: &str, args: &[&str]) -> CommandSpec {
        CommandSpec::new(
            std::iter::once(program.to_string())
                .chain(args.iter().map(|arg| (*arg).to_string()))
                .collect(),
        )
        .with_open_terminal(false)
    }

    #[tokio::test]
    async fn successful_commands_report_their_exit_code_and_output() {
        let runner = CommandRunner::new();
        let token = CancellationToken::new();
        let output = runner
            .run(
                &background("/bin/sh", &["-c", "printf hello; printf world >&2"]),
                &token,
                None,
            )
            .await;

        assert_eq!(output.status, RunStatus::Succeeded { exit_code: 0 });
        assert_eq!(output.stdout, "hello");
        assert_eq!(output.stderr, "world");
    }

    #[tokio::test]
    async fn failing_commands_report_a_nonzero_exit_code() {
        let runner = CommandRunner::new();
        let token = CancellationToken::new();
        let output = runner
            .run(&background("/bin/sh", &["-c", "exit 3"]), &token, None)
            .await;

        assert_eq!(output.status, RunStatus::Failed { exit_code: 3 });
    }

    #[tokio::test]
    async fn timeout_kills_a_runaway_command() {
        let runner = CommandRunner::new();
        let token = CancellationToken::new();
        let spec =
            background("/bin/sh", &["-c", "sleep 30"]).with_timeout(Duration::from_millis(150));

        let start = std::time::Instant::now();
        let output = runner.run(&spec, &token, None).await;

        assert_eq!(output.status, RunStatus::TimedOut);
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "the timeout must actually kill the child"
        );
    }

    #[tokio::test]
    async fn cancellation_kills_a_running_command() {
        let runner = CommandRunner::new();
        let token = CancellationToken::new();
        let spec = background("/bin/sh", &["-c", "sleep 30"]);

        let task = tokio::spawn({
            let runner = runner.clone();
            let token = token.clone();
            let spec = spec.clone();
            async move { runner.run(&spec, &token, None).await }
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        token.cancel();

        let output = task.await.expect("runner task joins");
        assert_eq!(output.status, RunStatus::Cancelled);
    }

    #[tokio::test]
    async fn a_direct_imported_confirmation_run_is_refused_until_approved() {
        let runner = CommandRunner::new();
        let token = CancellationToken::new();
        let spec = CommandSpec::new(vec!["rm".into(), "-rf".into(), "/tmp/nonexistent".into()])
            .with_imported(true)
            .with_open_terminal(false);

        let first = runner.run(&spec, &token, None).await;
        let RunStatus::ApprovalRequired(review_id) = first.status else {
            panic!("an imported confirmation command must not run before approval");
        };
        assert_eq!(first.stdout, "");

        let approved = runner.approve(&review_id).expect("approval succeeds");
        assert_eq!(approved, spec);

        let second = runner.run(&spec, &token, None).await;
        assert_eq!(second.status, RunStatus::Succeeded { exit_code: 0 });
    }

    #[tokio::test]
    async fn approval_is_bound_to_the_complete_spec_not_the_rendered_command() {
        let runner = CommandRunner::new();
        let token = CancellationToken::new();

        let base = CommandSpec::new(vec!["rm".into(), "-rf".into(), "/tmp/nonexistent".into()])
            .with_imported(true)
            .with_open_terminal(false);
        let first = runner.run(&base, &token, None).await;
        let RunStatus::ApprovalRequired(review_id) = first.status else {
            panic!("expected approval to be required");
        };
        runner.approve(&review_id).expect("approval succeeds");

        // Mutating any security-relevant field makes a different spec that
        // must be approved again.
        for mutated in [
            base.clone().with_timeout(Duration::from_secs(1)),
            base.clone()
                .with_cwd(crate::CwdStrategy::Fixed("/tmp".into())),
            base.clone()
                .with_env(crate::SanitizedEnv::new().with_var("EXTRA", "1")),
            base.clone().with_open_terminal(true),
        ] {
            let outcome = runner.run(&mutated, &token, None).await;
            assert!(
                matches!(outcome.status, RunStatus::ApprovalRequired(_)),
                "a mutated spec must be reviewed again"
            );
        }

        // The unmutated spec stays approved.
        assert!(runner.is_approved(&base));
        let again = runner.run(&base, &token, None).await;
        assert_eq!(again.status, RunStatus::Succeeded { exit_code: 0 });
    }

    #[tokio::test]
    async fn cancelled_commands_reap_their_descendants() {
        let dir = tempfile::tempdir().expect("temp dir");
        let sentinel = dir.path().join("sentinel");
        let script = format!(
            "(/bin/sleep 0.2; echo boom > {}) & /bin/sleep 30",
            sentinel.display()
        );
        let runner = CommandRunner::new();
        let token = CancellationToken::new();
        let spec =
            CommandSpec::new(vec!["/bin/sh".into(), "-c".into(), script]).with_open_terminal(false);

        let task = tokio::spawn({
            let runner = runner.clone();
            let token = token.clone();
            let spec = spec.clone();
            async move { runner.run(&spec, &token, None).await }
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        token.cancel();

        let output = task.await.expect("runner task joins");
        assert_eq!(output.status, RunStatus::Cancelled);

        // The grandchild would write the sentinel ~200ms in. Give it well past
        // that window; a surviving descendant must not be able to.
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(
            !sentinel.exists(),
            "a descendant of a cancelled command must not outlive it"
        );
    }

    #[tokio::test]
    async fn the_sanitized_environment_is_applied_to_the_child() {
        let runner = CommandRunner::new();
        let token = CancellationToken::new();
        let spec = background(
            "/bin/sh",
            &["-c", "printf %s \"$HOTWIRE_MODE\" \"$UNINHERITED\""],
        )
        .with_cwd(crate::CwdStrategy::Home)
        .with_env(crate::SanitizedEnv::new().with_var("HOTWIRE_MODE", "safe"))
        .with_open_terminal(false);

        let output = runner.run(&spec, &token, None).await;
        assert_eq!(output.status, RunStatus::Succeeded { exit_code: 0 });
        assert_eq!(output.stdout, "safe");
    }

    #[tokio::test]
    async fn missing_commands_fail_cleanly() {
        let runner = CommandRunner::new();
        let token = CancellationToken::new();
        let output = runner
            .run(
                &background("/definitely/not/a/real/binary", &[]),
                &token,
                None,
            )
            .await;

        assert!(matches!(output.status, RunStatus::StartError(_)));
    }

    #[tokio::test]
    async fn ask_strategy_refuses_to_pick_a_directory() {
        let runner = CommandRunner::new();
        let token = CancellationToken::new();
        let spec = background("/bin/true", &[]).with_cwd(crate::CwdStrategy::Ask);

        let output = runner.run(&spec, &token, None).await;
        assert!(matches!(output.status, RunStatus::StartError(_)));
    }

    #[test]
    fn visible_terminal_is_the_default_for_development_commands() {
        assert!(CommandSpec::new(vec!["make".into()]).open_terminal);
    }

    #[cfg(target_os = "macos")]
    mod macos_visible_terminal {
        use super::*;

        fn spec(argv: &[&str]) -> CommandSpec {
            CommandSpec::new(argv.iter().map(ToString::to_string).collect())
        }

        #[test]
        fn sh_quote_escapes_single_quotes_and_preserves_everything_else() {
            assert_eq!(sh_quote("plain"), "'plain'");
            assert_eq!(sh_quote("two words"), "'two words'");
            assert_eq!(sh_quote("it's"), "'it'\\''s'");
            assert_eq!(sh_quote("$(rm -rf /)"), "'$(rm -rf /)'");
            assert_eq!(sh_quote("line1\nline2"), "'line1\nline2'");
        }

        #[test]
        fn sh_quote_round_trips_through_a_real_shell_without_expansion() {
            for arg in [
                "two words",
                "it's",
                "$(echo PWNED)",
                "$HOME",
                "line1\nline2",
            ] {
                let token = sh_quote(arg);
                let script = format!("printf %s {token}");
                let output = std::process::Command::new("/bin/sh")
                    .arg("-c")
                    .arg(&script)
                    .output()
                    .expect("shell runs");
                assert!(
                    output.status.success(),
                    "script {script:?} must be valid shell"
                );
                assert_eq!(
                    String::from_utf8_lossy(&output.stdout),
                    arg,
                    "sh_quote({arg:?}) must survive a real shell unchanged"
                );
            }
        }

        #[test]
        fn terminal_script_carries_only_the_sanitized_environment() {
            let mut env = crate::SanitizedEnv::new().with_var("HOTWIRE_MODE", "safe");
            env.mark_secret("API_TOKEN");
            let spec = spec(&["/bin/echo", "hi"]).with_env(env);
            let script = build_terminal_script(&spec, None);

            assert_eq!(script, "exec env -i 'HOTWIRE_MODE=safe' '/bin/echo' 'hi'");
        }

        #[test]
        fn terminal_script_quotes_the_working_directory() {
            let spec = spec(&["/bin/pwd"]);
            let script = build_terminal_script(&spec, Some(Path::new("/tmp/My Dir/$HOME")));

            assert_eq!(script, "cd '/tmp/My Dir/$HOME'; exec env -i '/bin/pwd'");
        }

        #[test]
        fn terminal_script_quotes_every_argument_token() {
            let spec = spec(&["/bin/sh", "-c", "echo \"$HOME\" $(whoami)"]);
            let script = build_terminal_script(&spec, None);

            assert!(script.starts_with("exec env -i "));
            assert_eq!(
                script,
                "exec env -i '/bin/sh' '-c' 'echo \"$HOME\" $(whoami)'"
            );
        }

        #[test]
        fn applescript_quote_escapes_quotes_and_backslashes() {
            assert_eq!(applescript_quote("plain"), "\"plain\"");
            assert_eq!(applescript_quote("say \"hi\""), "\"say \\\"hi\\\"\"");
            assert_eq!(applescript_quote("a\\b"), "\"a\\\\b\"");
        }
    }
}
