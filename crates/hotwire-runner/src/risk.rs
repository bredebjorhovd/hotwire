//! Command risk classification (spec §15.2).
//!
//! Every command is classified so the review boundary knows when an exact
//! command must be shown and approved before execution. Classification is
//! deliberately *conservative*: destructive programs, destructive argument
//! forms on otherwise-approved CLIs, and destructive payloads passed to shell
//! interpreters (`sh`/`bash`/`zsh` `-c`) are all confirmation-risk, even when
//! user-authored. An arbitrary executable from an imported profile is
//! confirmation-risk unless it is on the approved-CLI list.

use serde::{Deserialize, Serialize};

use crate::command::CommandSpec;

/// Risk of an action or command, mirroring the catalog's `ActionDefinition`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    /// Focus an application, open a URL — no command is involved.
    None,
    /// Send a shortcut, start an approved CLI, or run a non-destructive command.
    Low,
    /// Destructive shell command, or an arbitrary script from an imported profile.
    Confirmation,
}

/// Programs that can destroy data or the machine; always confirmation-risk.
pub const DESTRUCTIVE_PROGRAMS: &[&str] = &[
    "rm",
    "rmdir",
    "dd",
    "mkfs",
    "mkfs.ext4",
    "mkfs.hfsplus",
    "mkfs.apfs",
    "diskutil",
    "fdisk",
    "mkfile",
    "shutdown",
    "reboot",
    "halt",
    "poweroff",
    "pkill",
    "killall",
    "chmod",
    "chown",
];

/// Shell interpreters whose `-c` payload is scanned for destructive commands.
const SHELL_INTERPRETERS: &[&str] = &["sh", "bash", "zsh", "dash", "ksh", "fish"];

/// Programs an imported profile may start without confirmation (spec §15.2's
/// "start approved CLI" category).
pub const APPROVED_CLIS: &[&str] = &[
    "open", "git", "ls", "cat", "pwd", "echo", "true", "false", "mkdir", "cp", "mv", "herdr",
    "claude", "codex",
];

/// Classifies the risk of running `spec`.
///
/// # Panics
///
/// Panics when `spec.argv` is empty; call [`CommandSpec::validate`] first.
#[must_use]
pub fn classify_command_risk(spec: &CommandSpec) -> RiskLevel {
    classify_argv(&spec.argv, spec.imported)
}

/// Classifies the risk of running `argv` (an argument array) with the given
/// provenance.
///
/// # Panics
///
/// Panics when `argv` is empty.
#[must_use]
pub fn classify_argv(argv: &[String], imported: bool) -> RiskLevel {
    let program = argv
        .first()
        .expect("a valid command has a program as its first argument");
    let base = program.rsplit('/').next().unwrap_or(program);
    let command_args = &argv[1..];

    // A destructive payload passed to a shell interpreter is confirmation-risk
    // regardless of provenance.
    if SHELL_INTERPRETERS.contains(&base)
        && command_args.first().is_some_and(|flag| flag == "-c")
        && command_args
            .get(1)
            .is_some_and(|payload| shell_payload_is_destructive(payload))
    {
        return RiskLevel::Confirmation;
    }

    // A destructive program name is always confirmation-risk.
    if DESTRUCTIVE_PROGRAMS.contains(&base) {
        return RiskLevel::Confirmation;
    }

    // Destructive argument forms on otherwise-approved CLIs are
    // confirmation-risk (e.g. `git clean -fdx`, `cp -f`, `mv --force`).
    if destructive_cli_form(base, command_args) {
        return RiskLevel::Confirmation;
    }

    // An arbitrary executable from an imported profile needs review.
    if imported && !APPROVED_CLIS.contains(&base) {
        return RiskLevel::Confirmation;
    }

    RiskLevel::Low
}

/// Whether a shell `-c` payload mentions a destructive operation in command
/// position (start of the payload or right after a `;`, `&`, `|`, or `(`).
///
/// This is a conservative token scan, not a shell parser: a false positive just
/// prompts an approval, while a false negative could destroy data. Payloads
/// that mention destructive programs or destructive git forms are treated as
/// confirmation-risk.
fn shell_payload_is_destructive(payload: &str) -> bool {
    let words: Vec<&str> = payload.split_whitespace().collect();
    let mut at_command_start = true;
    for (index, raw) in words.iter().enumerate() {
        let word = raw.trim_matches(|c| matches!(c, '\'' | '"' | '`'));
        let base = word.rsplit('/').next().unwrap_or(word);
        if at_command_start {
            if DESTRUCTIVE_PROGRAMS.contains(&base) {
                return true;
            }
            if base == "git" && git_payload_is_destructive(words.iter().copied().skip(index + 1)) {
                return true;
            }
        }
        at_command_start = raw.chars().any(|c| matches!(c, ';' | '&' | '|' | '('));
    }
    false
}

/// Whether a `git ...` payload is destructive (destructive subcommand forms).
fn git_payload_is_destructive<'a>(rest: impl IntoIterator<Item = &'a str>) -> bool {
    let rest: Vec<&str> = rest
        .into_iter()
        .map(|word| word.trim_matches(|c| c == '\'' || c == '"'))
        .collect();
    match rest.first().copied().unwrap_or("") {
        "clean" => rest.iter().any(|arg| arg.starts_with("-f")),
        "reset" => rest
            .iter()
            .any(|arg| matches!(*arg, "--hard" | "-f" | "--force")),
        "push" => rest.iter().any(|arg| matches!(*arg, "-f" | "--force")),
        _ => false,
    }
}

/// Whether approved-CLI arguments form a destructive operation.
fn destructive_cli_form(base: &str, args: &[String]) -> bool {
    match base {
        "git" => git_payload_is_destructive(args.iter().map(String::as_str)),
        "cp" | "mv" => args
            .iter()
            .any(|arg| arg.starts_with('-') && arg.contains('f')),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(program: &str, args: &[&str], imported: bool) -> CommandSpec {
        let mut full_argv = vec![program.to_string()];
        full_argv.extend(args.iter().map(|arg| (*arg).to_string()));
        CommandSpec::new(full_argv).with_imported(imported)
    }

    #[test]
    fn destructive_programs_are_always_confirmation_risk() {
        for program in [
            "rm", "rmdir", "dd", "mkfs", "diskutil", "shutdown", "pkill", "chmod",
        ] {
            assert_eq!(
                classify_command_risk(&command(program, &["-rf", "/"], false)),
                RiskLevel::Confirmation,
                "{program} must be confirmation-risk even when user-authored"
            );
        }
    }

    #[test]
    fn arbitrary_imported_executables_are_confirmation_risk() {
        assert_eq!(
            classify_command_risk(&command("./deploy.sh", &[], true)),
            RiskLevel::Confirmation
        );
        assert_eq!(
            classify_command_risk(&command("/tmp/setup", &["--yes"], true)),
            RiskLevel::Confirmation
        );
        assert_eq!(
            classify_command_risk(&command("ruby", &["/tmp/script.rb"], true)),
            RiskLevel::Confirmation
        );
        assert_eq!(
            classify_command_risk(&command("ruby", &["-e", "puts 1"], true)),
            RiskLevel::Confirmation,
            "an unknown imported interpreter form must fail toward confirmation"
        );
    }

    #[test]
    fn approved_clis_from_imported_profiles_are_low_risk() {
        for program in ["open", "git", "ls", "cat", "pwd", "echo", "herdr", "claude"] {
            assert_eq!(
                classify_command_risk(&command(program, &["status"], true)),
                RiskLevel::Low,
                "{program} must be a low-risk approved CLI"
            );
        }
    }

    #[test]
    fn destructive_shell_payloads_are_confirmation_risk_even_when_user_authored() {
        for program in ["sh", "bash", "zsh"] {
            assert_eq!(
                classify_command_risk(&command(program, &["-c", "rm -rf /tmp/x"], false)),
                RiskLevel::Confirmation,
                "{program} -c with a destructive payload must be confirmation-risk"
            );
            assert_eq!(
                classify_command_risk(&command(program, &["-c", "cd /tmp && dd if=/dev/zero of=x"], false)),
                RiskLevel::Confirmation,
                "{program} -c with a destructive payload after a separator must be confirmation-risk"
            );
        }
        // Absolute interpreter paths are detected too.
        assert_eq!(
            classify_command_risk(&command("/bin/sh", &["-c", "rm -rf /tmp/x"], false)),
            RiskLevel::Confirmation
        );
    }

    #[test]
    fn non_destructive_shell_payloads_stay_low() {
        assert_eq!(
            classify_command_risk(&command("sh", &["-c", "echo hi"], false)),
            RiskLevel::Low
        );
        assert_eq!(
            classify_command_risk(&command("sh", &["-c", "echo rm"], false)),
            RiskLevel::Low,
            "a destructive word not in command position is not an invocation"
        );
    }

    #[test]
    fn destructive_git_forms_are_confirmation_risk() {
        assert_eq!(
            classify_command_risk(&command("git", &["clean", "-fdx"], true)),
            RiskLevel::Confirmation
        );
        assert_eq!(
            classify_command_risk(&command("git", &["reset", "--hard", "HEAD~1"], false)),
            RiskLevel::Confirmation
        );
        assert_eq!(
            classify_command_risk(&command("git", &["push", "--force"], false)),
            RiskLevel::Confirmation
        );
        assert_eq!(
            classify_command_risk(&command("git", &["status"], true)),
            RiskLevel::Low
        );
    }

    #[test]
    fn destructive_cp_and_mv_forms_are_confirmation_risk() {
        assert_eq!(
            classify_command_risk(&command("cp", &["-rf", "/a", "/b"], true)),
            RiskLevel::Confirmation
        );
        assert_eq!(
            classify_command_risk(&command("mv", &["--force", "/a", "/b"], false)),
            RiskLevel::Confirmation
        );
        assert_eq!(
            classify_command_risk(&command("cp", &["-r", "/a", "/b"], true)),
            RiskLevel::Low
        );
    }

    #[test]
    fn user_authored_commands_are_low_risk_unless_destructive() {
        assert_eq!(
            classify_command_risk(&command("./deploy.sh", &[], false)),
            RiskLevel::Low
        );
        assert_eq!(
            classify_command_risk(&command("make", &["test"], false)),
            RiskLevel::Low
        );
    }
}
