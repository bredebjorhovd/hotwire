//! Command planning: argument arrays, working-directory strategies, and the
//! inspectable [`CommandSpec`] that a review screen shows verbatim before an
//! imported confirmation-risk command may run.

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use thiserror::Error;

use crate::env::SanitizedEnv;
use crate::risk::{classify_command_risk, RiskLevel};

/// Errors produced while planning a command execution.
#[derive(Debug, Error)]
pub enum CommandError {
    /// The command has no program to run.
    #[error("command must have a program as its first argument")]
    EmptyCommand,
    /// The timeout would immediately cancel the execution.
    #[error("timeout must be greater than zero")]
    ZeroTimeout,
    /// The command's working directory could not be resolved without user
    /// input (the `ask` strategy requires an interactive prompt).
    #[error("working directory could not be resolved (the `ask` strategy requires user input)")]
    CwdUnresolved,
    /// The environment carries an invalid key (empty or containing `=`/NUL).
    #[error("environment key `{0}` is not a valid environment variable name")]
    InvalidEnvironment(String),
}

/// Where a command runs, decided from the profile (spec §13.3).
#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum CwdStrategy {
    /// Always run in this directory.
    Fixed(PathBuf),
    /// Run in the user's home directory.
    Home,
    /// Run in the current project detected from a configured IDE integration.
    /// Resolution needs a project hint supplied at run time.
    CurrentProject,
    /// Ask the user every time; the runner refuses to pick a directory.
    Ask,
}

/// The result of resolving a [`CwdStrategy`] into something executable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolvedCwd {
    /// Run in this directory.
    Working(PathBuf),
    /// The `ask` strategy needs an interactive prompt; execution must pause
    /// until the user answers.
    AskUser,
}

impl CwdStrategy {
    /// Resolves the strategy into a working directory (or an explicit "ask
    /// the user" signal).
    ///
    /// `project` is the current-project hint used only by
    /// [`CwdStrategy::CurrentProject`]; other strategies ignore it.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError::CwdUnresolved`] when a concrete directory is
    /// required but unavailable: no home directory can be found, or no project
    /// hint was supplied for [`CwdStrategy::CurrentProject`].
    pub fn resolve(&self, project: Option<&std::path::Path>) -> Result<ResolvedCwd, CommandError> {
        match self {
            Self::Fixed(path) => Ok(ResolvedCwd::Working(path.clone())),
            Self::Home => match home::home_dir() {
                Some(home) => Ok(ResolvedCwd::Working(home)),
                None => Err(CommandError::CwdUnresolved),
            },
            Self::CurrentProject => match project {
                Some(path) => Ok(ResolvedCwd::Working(path.to_path_buf())),
                None => Err(CommandError::CwdUnresolved),
            },
            Self::Ask => Ok(ResolvedCwd::AskUser),
        }
    }
}

/// A planned command execution, inspectable before it runs.
///
/// The command is an argument array (spec §13.3): `argv[0]` is the program and
/// the remaining elements are arguments. There is no shell string to parse, so
/// quoting accidents and command injection cannot hide inside the command
/// line. For user-authored development commands the default is to run in a
/// visible terminal so the user watches exactly what executes.
///
/// `Eq`/`Ord` are full structural equality over every security-relevant field,
/// which is what approval is bound to. `Debug` is redacted: the environment
/// masks secret values and argv is passed through the env's redactor, so a
/// secret value used as an argument does not appear in derived output.
#[derive(Clone, Eq, PartialEq, PartialOrd, Ord)]
pub struct CommandSpec {
    /// The program and its arguments. `argv[0]` is the executable.
    pub argv: Vec<String>,
    /// Where the command runs.
    pub cwd: CwdStrategy,
    /// The sanitized environment: explicit variables plus an allowlist of host
    /// variables to inherit, with secret keys tracked for redaction.
    pub env: SanitizedEnv,
    /// How long the command may run before the runner kills it.
    pub timeout: Duration,
    /// Run the command in a visible terminal instead of capturing it.
    pub open_terminal: bool,
    /// Whether this command came from an imported profile. Imported commands
    /// that are not on the approved list are confirmation-risk and require
    /// review (spec §15.2).
    pub imported: bool,
}

impl fmt::Debug for CommandSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let redactor = self.env.redactor();
        formatter
            .debug_struct("CommandSpec")
            .field(
                "argv",
                &self
                    .argv
                    .iter()
                    .map(|arg| redactor.redact(arg))
                    .collect::<Vec<_>>(),
            )
            .field("cwd", &self.cwd)
            .field("env", &self.env)
            .field("timeout", &self.timeout)
            .field("open_terminal", &self.open_terminal)
            .field("imported", &self.imported)
            .finish()
    }
}

impl CommandSpec {
    /// Creates a spec that runs `argv` with a sane default timeout and a
    /// visible terminal (spec §13.3 default).
    ///
    /// # Panics
    ///
    /// Panics if `argv` is empty; use [`CommandSpec::validate`] on untrusted
    /// input instead.
    #[must_use]
    pub fn new(argv: Vec<String>) -> Self {
        assert!(!argv.is_empty(), "argv must not be empty");
        Self {
            argv,
            cwd: CwdStrategy::Home,
            env: SanitizedEnv::new(),
            timeout: Duration::from_secs(30),
            open_terminal: true,
            imported: false,
        }
    }

    /// Sets the working-directory strategy.
    #[must_use]
    pub fn with_cwd(mut self, cwd: CwdStrategy) -> Self {
        self.cwd = cwd;
        self
    }

    /// Sets the execution timeout.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Marks the command as imported from an external profile.
    #[must_use]
    pub fn with_imported(mut self, imported: bool) -> Self {
        self.imported = imported;
        self
    }

    /// Runs the command in a visible terminal instead of capturing it.
    #[must_use]
    pub fn with_open_terminal(mut self, open_terminal: bool) -> Self {
        self.open_terminal = open_terminal;
        self
    }

    /// Replaces the sanitized environment.
    #[must_use]
    pub fn with_env(mut self, env: SanitizedEnv) -> Self {
        self.env = env;
        self
    }

    /// Returns the command's risk level ([`RiskLevel::Confirmation`] for
    /// destructive programs and arbitrary imported executables).
    #[must_use]
    pub fn risk(&self) -> RiskLevel {
        classify_command_risk(self)
    }

    /// Returns the exact command line shown to the user in the review screen.
    #[must_use]
    pub fn describe(&self) -> String {
        self.argv
            .iter()
            .map(|arg| describe_arg(arg))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Resolves the working directory for this command.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError::CwdUnresolved`] when the strategy needs a
    /// directory that cannot be found or needs a user prompt.
    pub fn resolve_cwd(
        &self,
        project: Option<&std::path::Path>,
    ) -> Result<ResolvedCwd, CommandError> {
        self.cwd.resolve(project)
    }

    /// Validates that the spec is safe to plan.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError::EmptyCommand`] when `argv` is empty,
    /// [`CommandError::ZeroTimeout`] when the timeout is zero, and
    /// [`CommandError::InvalidEnvironment`] when an environment key is not a
    /// valid variable name.
    pub fn validate(&self) -> Result<(), CommandError> {
        if self.argv.is_empty() {
            return Err(CommandError::EmptyCommand);
        }
        if self.timeout.is_zero() {
            return Err(CommandError::ZeroTimeout);
        }
        self.env.validate()?;
        Ok(())
    }
}

fn describe_arg(arg: &str) -> String {
    if arg.is_empty()
        || arg
            .chars()
            .any(|c| c.is_whitespace() || c == '"' || c == '\'' || c == '\\' || c == '$')
    {
        format!("{arg:?}")
    } else {
        arg.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describe_renders_the_exact_command_for_review() {
        let spec = CommandSpec::new(vec!["open".into(), "Herdr app.app".into(), "--wait".into()])
            .with_cwd(CwdStrategy::Fixed(PathBuf::from("/tmp")));

        assert_eq!(spec.describe(), "open \"Herdr app.app\" --wait");
    }

    #[test]
    fn development_commands_open_a_visible_terminal_by_default() {
        let spec = CommandSpec::new(vec!["make".into(), "test".into()]);

        assert!(spec.open_terminal);
        assert!(!spec.imported);
        assert_eq!(spec.cwd, CwdStrategy::Home);
    }

    #[test]
    fn validation_rejects_empty_commands_and_zero_timeouts() {
        assert!(matches!(
            CommandSpec::new(vec!["echo".into()])
                .with_timeout(Duration::ZERO)
                .validate(),
            Err(CommandError::ZeroTimeout)
        ));

        let mut empty = CommandSpec::new(vec!["echo".into()]);
        empty.argv.clear();
        assert!(matches!(empty.validate(), Err(CommandError::EmptyCommand)));
    }

    #[test]
    fn fixed_and_current_project_strategies_resolve() {
        let fixed = CommandSpec::new(vec!["echo".into()])
            .with_cwd(CwdStrategy::Fixed(PathBuf::from("/tmp")));
        assert_eq!(
            fixed.resolve_cwd(None).expect("fixed resolves"),
            ResolvedCwd::Working(PathBuf::from("/tmp"))
        );

        let project = CommandSpec::new(vec!["echo".into()]).with_cwd(CwdStrategy::CurrentProject);
        assert_eq!(
            project
                .resolve_cwd(Some(PathBuf::from("/repo").as_path()))
                .expect("project resolves with a hint"),
            ResolvedCwd::Working(PathBuf::from("/repo"))
        );
        assert!(matches!(
            project.resolve_cwd(None),
            Err(CommandError::CwdUnresolved)
        ));
    }

    #[test]
    fn ask_strategy_defers_to_the_user() {
        let spec = CommandSpec::new(vec!["echo".into()]).with_cwd(CwdStrategy::Ask);

        assert_eq!(
            spec.resolve_cwd(None).expect("ask defers"),
            ResolvedCwd::AskUser
        );
    }

    #[test]
    fn debug_output_redacts_secret_values_in_env_and_argv() {
        let mut env = crate::env::SanitizedEnv::new().with_var("API_TOKEN", "ghp-super-secret");
        env.mark_secret("API_TOKEN");
        let spec = CommandSpec::new(vec!["echo".into(), "ghp-super-secret".into()]).with_env(env);

        let debug = format!("{spec:?}");
        assert!(
            !debug.contains("ghp-super-secret"),
            "derived Debug output must not leak a secret value"
        );
        assert!(debug.contains("[REDACTED]"));
    }
}
