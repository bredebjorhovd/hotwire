//! Process execution with timeouts and cancellation.
//!
//! [`CommandRunner`] spawns a [`CommandSpec`] as an argument array with a
//! sanitized environment, a resolved working directory, a hard timeout, and a
//! shared [`CancellationToken`](crate::CancellationToken). When the spec asks
//! for a visible terminal (the default for development commands, spec §13.3)
//! the command is handed to a real terminal session and its output is not
//! captured; otherwise the runner tracks the child, captures its output up to a
//! cap, and kills it on timeout or cancellation. Spawning and waiting always
//! happen on a tokio task — never on a native input callback thread.

use std::path::Path;
use std::process::Stdio;

use tokio::io::{AsyncRead, AsyncReadExt};

use crate::command::CommandSpec;
use crate::command::{CommandError, ResolvedCwd};
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
    /// The command was cancelled before finishing.
    Cancelled,
    /// The command exceeded its timeout and was killed.
    TimedOut,
    /// The command was handed to a visible terminal; its outcome is not tracked here.
    SpawnedInTerminal,
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

/// Runs [`CommandSpec`]s. Stateless and cheap; share one per app.
#[derive(Clone, Debug, Default)]
pub struct CommandRunner;

impl CommandRunner {
    /// Creates a runner.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Runs `spec`, cancelling the child when `token` is cancelled or the spec
    /// timeout elapses.
    ///
    /// `project` supplies the current-project hint used by the
    /// [`CwdStrategy::CurrentProject`](crate::CwdStrategy) strategy.
    ///
    /// A visible-terminal command returns
    /// [`RunStatus::SpawnedInTerminal`] immediately. A background command is
    /// tracked to completion, timeout, or cancellation.
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
            return match spawn_visible_terminal(spec, cwd.as_deref()) {
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

/// Spawns the command in a visible terminal on the current platform.
#[cfg(target_os = "macos")]
fn spawn_visible_terminal(spec: &CommandSpec, cwd: Option<&Path>) -> Result<(), String> {
    let mut script = String::new();
    if let Some(dir) = cwd {
        script.push_str("cd ");
        script.push_str(&shell_quote(&dir.to_string_lossy()));
        script.push_str(" && ");
    }
    script.push_str(&spec.describe());
    let apple = format!(
        "tell application \"Terminal\" to do script {}",
        shell_quote(&script)
    );
    std::process::Command::new("osascript")
        .arg("-e")
        .arg(&apple)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("failed to open a visible terminal: {error}"))
}

/// Spawns the command directly as a fallback where no terminal wrapper exists.
#[cfg(not(target_os = "macos"))]
fn spawn_visible_terminal(spec: &CommandSpec, cwd: Option<&Path>) -> Result<(), String> {
    let mut command = std::process::Command::new(&spec.argv[0]);
    command.args(&spec.argv[1..]);
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    command.env_clear();
    for (key, value) in spec.env.build() {
        command.env(key, value);
    }
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("failed to spawn: {error}"))
}

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
            let _ = child.kill().await;
            let _ = child.wait().await;
            let _ = stdout.await;
            let _ = stderr.await;
            CommandOutput {
                status: RunStatus::Cancelled,
                stdout: String::new(),
                stderr: String::new(),
            }
        }
        () = &mut timeout => {
            let _ = child.kill().await;
            let _ = child.wait().await;
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

/// Quotes a string for a POSIX shell inside an `AppleScript` string.
fn shell_quote(input: &str) -> String {
    format!("'{}'", input.replace('\'', "'\\''"))
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
}
