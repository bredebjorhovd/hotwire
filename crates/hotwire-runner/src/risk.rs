//! Command risk classification (spec §15.2).
//!
//! Every command is classified so the review boundary knows when an exact
//! command must be shown and approved before execution. Destructive programs
//! are always confirmation-risk; an arbitrary executable from an imported
//! profile is confirmation-risk unless it is on the approved-CLI list; anything
//! else is low risk.

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
];

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
    let program = spec
        .argv
        .first()
        .expect("a valid command has a program as its first argument");
    let base = program.rsplit('/').next().unwrap_or(program);

    if DESTRUCTIVE_PROGRAMS.contains(&base) {
        return RiskLevel::Confirmation;
    }
    if spec.imported && !APPROVED_CLIS.contains(&base) {
        return RiskLevel::Confirmation;
    }
    RiskLevel::Low
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
        for program in ["rm", "rmdir", "dd", "mkfs", "diskutil", "shutdown", "pkill"] {
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
