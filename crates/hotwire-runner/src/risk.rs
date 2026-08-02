//! Command risk classification (spec §15.2).
//!
//! Every command is classified so the review boundary knows when an exact
//! command must be shown and approved before execution. Classification is
//! deliberately *fail-closed*: destructive programs, destructive argument
//! forms on otherwise-approved CLIs, and *any* shell interpreter command-string
//! form (`sh`/`bash`/`zsh` `-c`, including combined options like `-lc`) are
//! confirmation-risk, even when user-authored — a shell payload is arbitrary
//! code and cannot be safely reasoned about by name. An arbitrary executable
//! from an imported profile is confirmation-risk unless it is on the
//! approved-CLI list.

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

/// Shell interpreters whose command-string form (`-c`) is fail-closed
/// confirmation-risk.
const SHELL_INTERPRETERS: &[&str] = &["sh", "bash", "zsh", "dash", "ksh", "fish"];

/// Programs an imported profile may start without confirmation (spec §15.2's
/// "start approved CLI" category).
pub const APPROVED_CLIS: &[&str] = &[
    "open", "git", "ls", "cat", "pwd", "echo", "true", "false", "mkdir", "cp", "mv", "herdr",
    "claude", "codex",
];

/// `git` subcommands safe to run without confirmation. Everything else is
/// confirmation-risk: `rm`, `clean`, `reset`, `push --force`, `branch -D`,
/// `restore`, `checkout -- <path>`, `stash drop`, and unknown subcommands all
/// fail closed.
const SAFE_GIT_SUBCOMMANDS: &[&str] = &["status", "log", "diff", "show", "fetch"];

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

    // Any shell interpreter command-string form (`-c`, including combined
    // options such as `-lc`) is confirmation-risk regardless of provenance:
    // the payload is arbitrary shell and cannot be reasoned about by name.
    if SHELL_INTERPRETERS.contains(&base)
        && command_args
            .iter()
            .any(|arg| arg.starts_with('-') && arg.contains('c'))
    {
        return RiskLevel::Confirmation;
    }

    // A destructive program name is always confirmation-risk.
    if DESTRUCTIVE_PROGRAMS.contains(&base) {
        return RiskLevel::Confirmation;
    }

    // Destructive argument forms on otherwise-approved CLIs are
    // confirmation-risk (`git` outside the safe-subcommand list, `cp`/`mv`
    // without a proven no-overwrite flag).
    if destructive_cli_form(base, command_args) {
        return RiskLevel::Confirmation;
    }

    // An arbitrary executable from an imported profile needs review.
    if imported && !APPROVED_CLIS.contains(&base) {
        return RiskLevel::Confirmation;
    }

    RiskLevel::Low
}

/// Whether approved-CLI arguments form a destructive operation.
fn destructive_cli_form(base: &str, args: &[String]) -> bool {
    match base {
        // Fail closed: only the safe-subcommand list is Low.
        "git" => !safe_git_subcommand(args),
        // `cp`/`mv` can overwrite an existing destination even without `-f`;
        // they are only Low when a proven no-overwrite flag is present.
        "cp" | "mv" => !has_no_overwrite_flag(args),
        _ => false,
    }
}

/// Whether the `git` invocation uses a safe subcommand.
///
/// The first non-option token is the subcommand; when it is absent or not in
/// the safe list, the invocation fails closed to confirmation-risk.
fn safe_git_subcommand(args: &[String]) -> bool {
    args.iter()
        .find(|arg| !arg.starts_with('-'))
        .map(String::as_str)
        .is_some_and(|subcommand| SAFE_GIT_SUBCOMMANDS.contains(&subcommand))
}

/// Whether `cp`/`mv` carry a proven no-overwrite flag (`-n`/`--no-clobber`).
///
/// `-i`/`--interactive` is *not* sufficient: an interactive overwrite prompt
/// can still overwrite, so only hard no-clobber counts as a safe form.
fn has_no_overwrite_flag(args: &[String]) -> bool {
    args.iter().any(|arg| {
        arg == "--no-clobber"
            || (arg.starts_with('-')
                && !arg.starts_with("--")
                && arg.len() > 1
                && arg.contains('n'))
    })
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
    fn any_shell_command_string_form_is_confirmation_risk_even_when_user_authored() {
        // Fail-closed: the payload is arbitrary shell, so every `-c` form is
        // confirmation-risk, not just the ones a parser recognizes.
        for program in ["sh", "bash", "zsh"] {
            assert_eq!(
                classify_command_risk(&command(program, &["-c", "echo hi"], false)),
                RiskLevel::Confirmation,
                "{program} -c must be confirmation-risk even with a benign payload"
            );
            assert_eq!(
                classify_command_risk(&command(program, &["-c", "rm -rf /tmp/x"], false)),
                RiskLevel::Confirmation,
                "{program} -c with a destructive payload must be confirmation-risk"
            );
            // Attached separators no longer matter: the whole form fails closed.
            assert_eq!(
                classify_command_risk(&command(program, &["-c", "cd /tmp;rm -rf x"], false)),
                RiskLevel::Confirmation,
                "{program} -c with an attached separator must be confirmation-risk"
            );
            // Combined option forms (`-lc`) must not bypass the `-c` check.
            assert_eq!(
                classify_command_risk(&command(program, &["-lc", "rm -rf x"], false)),
                RiskLevel::Confirmation,
                "{program} -lc must be confirmation-risk"
            );
        }
        // Absolute interpreter paths are detected too.
        assert_eq!(
            classify_command_risk(&command("/bin/sh", &["-c", "rm -rf /tmp/x"], false)),
            RiskLevel::Confirmation
        );
    }

    #[test]
    fn non_command_string_shell_forms_are_not_fail_closed() {
        // A script file (not a command string) is treated like any other
        // user-authored executable; only an imported unknown script is
        // confirmation-risk.
        assert_eq!(
            classify_command_risk(&command("sh", &["./deploy.sh"], false)),
            RiskLevel::Low
        );
        assert_eq!(
            classify_command_risk(&command("sh", &["-x", "script.sh"], false)),
            RiskLevel::Low
        );
        assert_eq!(
            classify_command_risk(&command("sh", &["-n", "script.sh"], false)),
            RiskLevel::Low
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
    fn git_outside_the_safe_subcommand_list_is_confirmation_risk() {
        for (args, label) in [
            (vec!["clean", "-fdx"], "git clean -fdx"),
            (vec!["reset", "--hard"], "git reset --hard"),
            (vec!["push", "--force"], "git push --force"),
            (vec!["rm", "file"], "git rm"),
            (vec!["branch", "-D", "topic"], "git branch -D"),
            (vec!["restore", "file"], "git restore"),
            (vec!["checkout", "--", "file"], "git checkout -- path"),
            (vec!["stash", "drop"], "git stash drop"),
            (vec!["pull"], "git pull"),
            (vec!["reset"], "git reset"),
        ] {
            let argv = std::iter::once("git".to_string())
                .chain(args.iter().map(|arg| (*arg).to_string()))
                .collect();
            let spec = CommandSpec::new(argv).with_imported(true);
            assert_eq!(
                classify_command_risk(&spec),
                RiskLevel::Confirmation,
                "{label} must be confirmation-risk"
            );
        }
    }

    #[test]
    fn git_safe_subcommands_are_low_risk() {
        for subcommand in ["status", "log", "diff", "show", "fetch"] {
            assert_eq!(
                classify_command_risk(&command("git", &[subcommand], true)),
                RiskLevel::Low,
                "git {subcommand} must be a low-risk safe subcommand"
            );
        }
    }

    #[test]
    fn cp_and_mv_require_a_proven_no_overwrite_form() {
        // Without `-n`/`--no-clobber`, cp/mv can overwrite an existing
        // destination, so they are confirmation-risk.
        assert_eq!(
            classify_command_risk(&command("cp", &["source", "dest"], true)),
            RiskLevel::Confirmation
        );
        assert_eq!(
            classify_command_risk(&command("mv", &["source", "dest"], true)),
            RiskLevel::Confirmation
        );
        assert_eq!(
            classify_command_risk(&command("cp", &["-rf", "/a", "/b"], true)),
            RiskLevel::Confirmation
        );
        assert_eq!(
            classify_command_risk(&command("cp", &["-r", "/a", "/b"], false)),
            RiskLevel::Confirmation,
            "recursive copy can still overwrite an existing destination"
        );
        // A proven no-overwrite flag is Low.
        assert_eq!(
            classify_command_risk(&command("cp", &["-rn", "/a", "/b"], true)),
            RiskLevel::Low
        );
        assert_eq!(
            classify_command_risk(&command("mv", &["--no-clobber", "/a", "/b"], false)),
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
