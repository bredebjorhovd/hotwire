//! Redacted local logs.
//!
//! Hotwire is quiet software: it logs only a closed set of diagnostic fields
//! (spec §15.1). A [`LogEntry`] has no room for typed text, prompts, arbitrary
//! key sequences, or free-form payloads; the only free-text field is
//! `message`, and [`SafetyLog`] pushes it through a [`Redactor`] before
//! anything is written, so secrets (spec §15.3) stay out of every sink. No
//! telemetry leaves the machine: these logs are local by construction.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::redact::Redactor;

/// How severe a log entry is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogLevel {
    /// Normal operational detail.
    Info,
    /// Something is degraded but recoverable.
    Warning,
    /// Something failed.
    Error,
}

/// Which Hotwire surface produced the entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogCategory {
    /// Input capture, permission, and tap health.
    Capture,
    /// Command/action execution.
    Execution,
    /// The review-before-execute approval flow.
    Approval,
    /// Pause/resume and shutdown recovery.
    Recovery,
    /// Diagnostic snapshots.
    Diagnostics,
}

/// How a lifecycle operation ended, kept deliberately parallel to the action
/// status model but independent so the runner stays dependency-free.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// The operation started and is in flight.
    Started,
    /// The operation finished successfully.
    Succeeded,
    /// The operation failed.
    Failed,
    /// The operation was cancelled.
    Cancelled,
}

/// One redacted, local log entry.
///
/// The field set is closed by design: identifiers for the action, adapter,
/// and matched physical code (a configured binding, never an arbitrary key
/// sequence) plus a redacted message. There is no field for typed text,
/// prompts, or raw events.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogEntry {
    /// Unix-millisecond timestamp.
    pub timestamp: u64,
    pub level: LogLevel,
    pub category: LogCategory,
    /// Outcome of the operation, when the entry reports one.
    pub outcome: Option<Outcome>,
    /// The semantic action id, e.g. `shell.run`.
    pub action_id: Option<String>,
    /// The adapter id that executed the action.
    pub adapter_id: Option<String>,
    /// The single matched physical code (a configured binding).
    pub physical_code: Option<String>,
    /// Free-text detail, redacted before it is written.
    pub message: String,
}

impl LogEntry {
    /// Creates an entry with no outcome or binding context.
    #[must_use]
    pub fn new(level: LogLevel, category: LogCategory, message: impl Into<String>) -> Self {
        Self {
            timestamp: now_millis(),
            level,
            category,
            outcome: None,
            action_id: None,
            adapter_id: None,
            physical_code: None,
            message: message.into(),
        }
    }

    /// Attaches an outcome to the entry.
    #[must_use]
    pub fn with_outcome(mut self, outcome: Outcome) -> Self {
        self.outcome = Some(outcome);
        self
    }

    /// Attaches action/adapter identifiers to the entry.
    #[must_use]
    pub fn with_action(
        mut self,
        action_id: impl Into<String>,
        adapter_id: impl Into<String>,
    ) -> Self {
        self.action_id = Some(action_id.into());
        self.adapter_id = Some(adapter_id.into());
        self
    }

    /// Attaches the single matched physical code to the entry.
    #[must_use]
    pub fn with_physical_code(mut self, physical_code: impl Into<String>) -> Self {
        self.physical_code = Some(physical_code.into());
        self
    }
}

/// A destination for redacted [`LogEntry`]s.
pub trait LogSink {
    /// Writes one entry. The implementation must not buffer indefinitely;
    /// [`SafetyLog`] flushes after every entry so logs survive crashes.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the sink cannot be written.
    fn write(&mut self, entry: &LogEntry) -> io::Result<()>;
}

/// An in-memory log sink for tests and diagnostics buffers.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InMemorySink {
    entries: Vec<LogEntry>,
}

impl InMemorySink {
    /// Creates an empty in-memory sink.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns every entry written so far.
    #[must_use]
    pub fn entries(&self) -> &[LogEntry] {
        &self.entries
    }

    /// Returns the redacted messages written so far.
    #[must_use]
    pub fn messages(&self) -> Vec<String> {
        self.entries
            .iter()
            .map(|entry| entry.message.clone())
            .collect()
    }

    /// Joins all written messages for whole-log assertions.
    #[must_use]
    pub fn joined(&self) -> String {
        self.messages().join("\n")
    }
}

impl LogSink for InMemorySink {
    fn write(&mut self, entry: &LogEntry) -> io::Result<()> {
        self.entries.push(entry.clone());
        Ok(())
    }
}

/// A file-backed log sink. Appends one JSON line per entry.
#[derive(Debug)]
pub struct FileSink {
    writer: BufWriter<File>,
}

impl FileSink {
    /// Creates (or truncates) a log file at `path`.
    ///
    /// # Errors
    ///
    /// Returns the underlying I/O error when the file cannot be created.
    pub fn create(path: impl AsRef<Path>) -> io::Result<Self> {
        let file = File::create(path)?;
        Ok(Self {
            writer: BufWriter::new(file),
        })
    }
}

impl LogSink for FileSink {
    fn write(&mut self, entry: &LogEntry) -> io::Result<()> {
        // The entry is redacted before it reaches us; serialize the closed
        // field set as a single JSON object so the file is machine-readable
        // and contains nothing but allowed fields.
        writeln!(self.writer, "{}", serialize_entry(entry))?;
        self.writer.flush()
    }
}

/// Renders an entry to a compact JSON line. Kept string-based so the sink can
/// stay dependency-free of a serializer.
fn serialize_entry(entry: &LogEntry) -> String {
    let mut out = String::from("{\"timestamp\":");
    out.push_str(&entry.timestamp.to_string());
    out.push_str(",\"level\":\"");
    out.push_str(match entry.level {
        LogLevel::Info => "info",
        LogLevel::Warning => "warning",
        LogLevel::Error => "error",
    });
    out.push_str("\",\"category\":\"");
    out.push_str(match entry.category {
        LogCategory::Capture => "capture",
        LogCategory::Execution => "execution",
        LogCategory::Approval => "approval",
        LogCategory::Recovery => "recovery",
        LogCategory::Diagnostics => "diagnostics",
    });
    out.push_str("\",\"outcome\":");
    out.push_str(match entry.outcome {
        Some(Outcome::Started) => "\"started\"",
        Some(Outcome::Succeeded) => "\"succeeded\"",
        Some(Outcome::Failed) => "\"failed\"",
        Some(Outcome::Cancelled) => "\"cancelled\"",
        None => "null",
    });
    out.push_str(",\"actionId\":");
    push_optional(&mut out, entry.action_id.as_deref());
    out.push_str(",\"adapterId\":");
    push_optional(&mut out, entry.adapter_id.as_deref());
    out.push_str(",\"physicalCode\":");
    push_optional(&mut out, entry.physical_code.as_deref());
    out.push_str(",\"message\":");
    push_optional(&mut out, Some(&entry.message));
    out.push('}');
    out
}

fn push_optional(out: &mut String, value: Option<&str>) {
    match value {
        Some(value) => {
            out.push('"');
            for c in value.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    _ => out.push(c),
                }
            }
            out.push('"');
        }
        None => out.push_str("null"),
    }
}

/// The redacted log boundary.
///
/// Every entry's message is run through the [`Redactor`] before the sink sees
/// it, so a message that carries a secret value or a secret-style `KEY=value`
/// token cannot reach any sink.
pub struct SafetyLog<S> {
    sink: S,
    redactor: Redactor,
}

impl SafetyLog<InMemorySink> {
    /// Creates an in-memory safety log.
    #[must_use]
    pub fn memory() -> Self {
        Self {
            sink: InMemorySink::new(),
            redactor: Redactor::new(),
        }
    }
}

impl SafetyLog<FileSink> {
    /// Creates a file-backed safety log.
    ///
    /// # Errors
    ///
    /// Returns the underlying I/O error when the file cannot be created.
    pub fn file(path: impl AsRef<Path>) -> io::Result<Self> {
        Ok(Self {
            sink: FileSink::create(path)?,
            redactor: Redactor::new(),
        })
    }
}

impl<S: LogSink> SafetyLog<S> {
    /// Wraps `sink` with the given redactor.
    #[must_use]
    pub fn with_redactor(sink: S, redactor: Redactor) -> Self {
        Self { sink, redactor }
    }

    /// Returns a reference to the underlying sink.
    #[must_use]
    pub fn sink(&self) -> &S {
        &self.sink
    }

    /// Registers a literal value to mask from every future message.
    pub fn add_redactor_literal(&mut self, literal: impl Into<String>) {
        self.redactor.add_literal(literal);
    }

    /// Registers an assignment key to mask from every future message.
    pub fn add_redactor_key(&mut self, key: impl Into<String>) {
        self.redactor.add_key(key);
    }

    /// Writes `entry`, redacting its message first.
    ///
    /// # Errors
    ///
    /// Returns the sink's I/O error when the entry cannot be written.
    pub fn log(&mut self, mut entry: LogEntry) -> io::Result<()> {
        entry.message = self.redactor.redact(&entry.message);
        self.sink.write(&entry)
    }

    /// Writes an info entry.
    ///
    /// # Errors
    ///
    /// Returns the sink's I/O error when the entry cannot be written.
    pub fn info(&mut self, category: LogCategory, message: impl Into<String>) -> io::Result<()> {
        self.log(LogEntry::new(LogLevel::Info, category, message))
    }

    /// Writes a warning entry.
    ///
    /// # Errors
    ///
    /// Returns the sink's I/O error when the entry cannot be written.
    pub fn warn(&mut self, category: LogCategory, message: impl Into<String>) -> io::Result<()> {
        self.log(LogEntry::new(LogLevel::Warning, category, message))
    }

    /// Writes an error entry.
    ///
    /// # Errors
    ///
    /// Returns the sink's I/O error when the entry cannot be written.
    pub fn error(&mut self, category: LogCategory, message: impl Into<String>) -> io::Result<()> {
        self.log(LogEntry::new(LogLevel::Error, category, message))
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_carry_only_a_closed_field_set() {
        let entry = LogEntry::new(LogLevel::Info, LogCategory::Execution, "focused herdr")
            .with_outcome(Outcome::Succeeded)
            .with_action("app.open_or_focus", "herdr")
            .with_physical_code("Numpad5");

        assert_eq!(entry.action_id.as_deref(), Some("app.open_or_focus"));
        assert_eq!(entry.adapter_id.as_deref(), Some("herdr"));
        assert_eq!(entry.physical_code.as_deref(), Some("Numpad5"));
        assert_eq!(entry.outcome, Some(Outcome::Succeeded));
    }

    #[test]
    fn secret_values_never_reach_a_sink() {
        let mut log = SafetyLog::memory();
        log.add_redactor_literal("sk-super-secret");

        log.info(
            LogCategory::Execution,
            "ran with key sk-super-secret attached",
        )
        .expect("log writes");

        let joined = log.sink().joined();
        assert!(
            !joined.contains("sk-super-secret"),
            "the secret value must not reach the sink"
        );
        assert!(joined.contains("[REDACTED]"));
    }

    #[test]
    fn secret_assignment_tokens_are_masked_by_default() {
        let mut log = SafetyLog::memory();

        log.info(
            LogCategory::Execution,
            "used ANTHROPIC_API_KEY=sk-leak here",
        )
        .expect("log writes");

        let joined = log.sink().joined();
        assert!(!joined.contains("sk-leak"));
        assert!(joined.contains("[REDACTED]"));
    }

    #[test]
    fn environment_secrets_are_redacted_through_the_log() {
        let mut env = crate::SanitizedEnv::new().with_var("GITHUB_TOKEN", "ghp_123456");
        env.mark_secret("GITHUB_TOKEN");
        let mut log = SafetyLog::with_redactor(InMemorySink::new(), env.redactor());

        log.info(
            LogCategory::Diagnostics,
            format!("env: {:?}", env.build_redacted()),
        )
        .expect("log writes");

        let joined = log.sink().joined();
        assert!(!joined.contains("ghp_123456"));
        assert!(joined.contains("[REDACTED]"));
    }

    #[test]
    fn entries_have_no_field_for_typed_text_prompts_or_key_sequences() {
        // The log model is a closed set of diagnostic identifiers plus a
        // redacted message. There is no field that can carry typed text, a
        // prompt, or an arbitrary key sequence, so a serialized entry can only
        // ever contain the allowlisted fields.
        let entry = LogEntry::new(LogLevel::Info, LogCategory::Capture, "note")
            .with_action("app.x", "ad")
            .with_physical_code("Numpad5");
        let json = serialize_entry(&entry);

        for allowed in [
            "timestamp",
            "level",
            "category",
            "outcome",
            "actionId",
            "adapterId",
            "physicalCode",
            "message",
        ] {
            assert!(
                json.contains(&format!("\"{allowed}\"")),
                "serialized entry must include the `{allowed}` field"
            );
        }
    }

    #[test]
    fn file_sink_serializes_redacted_json_lines() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("hotwire.log");
        let mut log = SafetyLog::file(&path).expect("file log");
        log.info(LogCategory::Recovery, "clean shutdown")
            .expect("writes");

        let contents = std::fs::read_to_string(&path).expect("read back");
        assert!(contents.contains("\"category\":\"recovery\""));
        assert!(contents.contains("\"message\":\"clean shutdown\""));
        assert!(contents.trim_end().ends_with('}'));
    }
}
