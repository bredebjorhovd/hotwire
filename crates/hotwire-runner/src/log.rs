//! Redacted, structured local logs.
//!
//! Hotwire is quiet software: it logs only a closed set of diagnostic fields
//! (spec §15.1), and it makes forbidden payloads *unrepresentable*. A
//! [`LogEntry`] carries allowlisted identifiers plus a structured
//! [`EventDetail`] — there is no free-text `message` field, so typed text,
//! prompts, file paths, and arbitrary key sequences cannot be written to a
//! persistent log at all, and secrets have no field to leak through. Raw-event
//! diagnostics live in a separate, explicitly opted-in, auto-expiring surface
//! ([`crate::RawEventDiagnostics`], spec §15.1) that is never persisted. No
//! telemetry leaves the machine: these logs are local by construction.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

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

/// Why a capture tap was disabled, as a fixed category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TapDisableReason {
    /// The system disabled the tap for a slow callback; Hotwire re-enables it.
    Timeout,
    /// The system disabled capture because the user entered secure input.
    SecureInput,
}

/// A structured, allowlisted record of what happened.
///
/// This is the *only* payload a persistent log entry can carry. Every variant
/// is a fixed category plus safe numeric or identifier fields — no free text,
/// no paths, no commands, no key sequences. Rendering a failure carries at
/// most an exit code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EventDetail {
    /// An execution began.
    ExecutionStarted,
    /// An execution succeeded.
    ExecutionSucceeded,
    /// An execution failed, with its exit code when one is known.
    ExecutionFailed { exit_code: Option<i32> },
    /// An execution was cancelled.
    ExecutionCancelled,
    /// An execution exceeded its timeout.
    ExecutionTimedOut,
    /// An imported confirmation-risk command awaits approval.
    ApprovalRequired { review_id: String },
    /// An approval was granted.
    ApprovalGranted { review_id: String },
    /// An approval was denied.
    ApprovalDenied { review_id: String },
    /// Capture was paused, cancelling this many in-flight executions.
    CapturePaused { cancelled: usize },
    /// Capture resumed.
    CaptureResumed,
    /// Clean shutdown cancelled this many in-flight executions.
    Shutdown { cancelled: usize },
    /// The input permission was lost; capture fails open.
    PermissionLost,
    /// The tap was disabled; keys pass through.
    TapDisabled { reason: TapDisableReason },
    /// The tap recovered after a transient disable.
    TapRecovered,
}

impl EventDetail {
    /// A stable `snake_case` kind for this detail, for filtering and serialization.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::ExecutionStarted => "execution_started",
            Self::ExecutionSucceeded => "execution_succeeded",
            Self::ExecutionFailed { .. } => "execution_failed",
            Self::ExecutionCancelled => "execution_cancelled",
            Self::ExecutionTimedOut => "execution_timed_out",
            Self::ApprovalRequired { .. } => "approval_required",
            Self::ApprovalGranted { .. } => "approval_granted",
            Self::ApprovalDenied { .. } => "approval_denied",
            Self::CapturePaused { .. } => "capture_paused",
            Self::CaptureResumed => "capture_resumed",
            Self::Shutdown { .. } => "shutdown",
            Self::PermissionLost => "permission_lost",
            Self::TapDisabled { .. } => "tap_disabled",
            Self::TapRecovered => "tap_recovered",
        }
    }
}

/// One structured, local log entry.
///
/// The field set is closed by design: identifiers for the action, adapter, and
/// the single matched physical code (a configured binding, never an arbitrary
/// key sequence) plus a structured [`EventDetail`]. There is no field for
/// typed text, prompts, paths, or raw events.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogEntry {
    /// Unix-millisecond timestamp.
    pub timestamp: u64,
    pub level: LogLevel,
    pub category: LogCategory,
    /// The semantic action id, e.g. `shell.run`.
    pub action_id: Option<String>,
    /// The adapter id that executed the action.
    pub adapter_id: Option<String>,
    /// The single matched physical code (a configured binding).
    pub physical_code: Option<String>,
    /// The structured record of what happened.
    pub detail: EventDetail,
}

impl LogEntry {
    /// Creates an entry with no binding context.
    #[must_use]
    pub fn new(level: LogLevel, category: LogCategory, detail: EventDetail) -> Self {
        Self {
            timestamp: now_millis(),
            level,
            category,
            action_id: None,
            adapter_id: None,
            physical_code: None,
            detail,
        }
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

/// A destination for structured [`LogEntry`]s.
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

    /// Returns the structured details written so far.
    #[must_use]
    pub fn details(&self) -> Vec<&EventDetail> {
        self.entries.iter().map(|entry| &entry.detail).collect()
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
        // Serialize the closed field set as a single JSON object so the file is
        // machine-readable and contains nothing but allowlisted fields.
        writeln!(self.writer, "{}", serialize_entry(entry))?;
        self.writer.flush()
    }
}

/// Renders an entry to a compact JSON line, kept string-based so the sink can
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
    out.push_str("\",\"actionId\":");
    push_optional(&mut out, entry.action_id.as_deref());
    out.push_str(",\"adapterId\":");
    push_optional(&mut out, entry.adapter_id.as_deref());
    out.push_str(",\"physicalCode\":");
    push_optional(&mut out, entry.physical_code.as_deref());
    out.push_str(",\"detail\":");
    out.push_str(&serialize_detail(&entry.detail));
    out.push('}');
    out
}

fn serialize_detail(detail: &EventDetail) -> String {
    let mut out = format!("{{\"kind\":\"{}\"", detail.kind());
    match detail {
        EventDetail::ExecutionFailed { exit_code } => {
            out.push_str(",\"exitCode\":");
            match exit_code {
                Some(code) => out.push_str(&code.to_string()),
                None => out.push_str("null"),
            }
        }
        EventDetail::ApprovalRequired { review_id }
        | EventDetail::ApprovalGranted { review_id }
        | EventDetail::ApprovalDenied { review_id } => {
            out.push_str(",\"reviewId\":");
            push_optional(&mut out, Some(review_id));
        }
        EventDetail::CapturePaused { cancelled } | EventDetail::Shutdown { cancelled } => {
            out.push_str(",\"cancelled\":");
            out.push_str(&cancelled.to_string());
        }
        EventDetail::TapDisabled { reason } => {
            out.push_str(",\"reason\":\"");
            out.push_str(match reason {
                TapDisableReason::Timeout => "timeout",
                TapDisableReason::SecureInput => "secure_input",
            });
            out.push('"');
        }
        EventDetail::ExecutionStarted
        | EventDetail::ExecutionSucceeded
        | EventDetail::ExecutionCancelled
        | EventDetail::ExecutionTimedOut
        | EventDetail::CaptureResumed
        | EventDetail::PermissionLost
        | EventDetail::TapRecovered => {}
    }
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

/// The structured log boundary.
///
/// Writes only allowlisted fields: entries cannot carry free text, so nothing
/// needs redacting and nothing sensitive can be persisted.
pub struct SafetyLog<S> {
    sink: S,
}

impl SafetyLog<InMemorySink> {
    /// Creates an in-memory safety log.
    #[must_use]
    pub fn memory() -> Self {
        Self {
            sink: InMemorySink::new(),
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
        })
    }
}

impl<S: LogSink> SafetyLog<S> {
    /// Returns a reference to the underlying sink.
    #[must_use]
    pub fn sink(&self) -> &S {
        &self.sink
    }

    /// Writes `entry`.
    ///
    /// # Errors
    ///
    /// Returns the sink's I/O error when the entry cannot be written.
    pub fn log(&mut self, entry: &LogEntry) -> io::Result<()> {
        self.sink.write(entry)
    }

    /// Writes an info entry.
    ///
    /// # Errors
    ///
    /// Returns the sink's I/O error when the entry cannot be written.
    pub fn info(&mut self, category: LogCategory, detail: EventDetail) -> io::Result<()> {
        self.log(&LogEntry::new(LogLevel::Info, category, detail))
    }

    /// Writes a warning entry.
    ///
    /// # Errors
    ///
    /// Returns the sink's I/O error when the entry cannot be written.
    pub fn warn(&mut self, category: LogCategory, detail: EventDetail) -> io::Result<()> {
        self.log(&LogEntry::new(LogLevel::Warning, category, detail))
    }

    /// Writes an error entry.
    ///
    /// # Errors
    ///
    /// Returns the sink's I/O error when the entry cannot be written.
    pub fn error(&mut self, category: LogCategory, detail: EventDetail) -> io::Result<()> {
        self.log(&LogEntry::new(LogLevel::Error, category, detail))
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
        let entry = LogEntry::new(
            LogLevel::Info,
            LogCategory::Execution,
            EventDetail::ExecutionSucceeded,
        )
        .with_action("app.open_or_focus", "herdr")
        .with_physical_code("Numpad5");

        assert_eq!(entry.action_id.as_deref(), Some("app.open_or_focus"));
        assert_eq!(entry.adapter_id.as_deref(), Some("herdr"));
        assert_eq!(entry.physical_code.as_deref(), Some("Numpad5"));
        assert_eq!(entry.detail, EventDetail::ExecutionSucceeded);
    }

    #[test]
    fn entries_have_no_field_for_typed_text_prompts_paths_or_key_sequences() {
        // The log model is a closed set of identifiers plus a structured
        // EventDetail; there is no free-text field at all. A serialized entry
        // can therefore only ever contain the allowlisted keys.
        let entry = LogEntry::new(
            LogLevel::Error,
            LogCategory::Execution,
            EventDetail::ExecutionFailed { exit_code: Some(1) },
        )
        .with_action("shell.run", "shell")
        .with_physical_code("Numpad5");
        let json = serialize_entry(&entry);

        for allowed in [
            "timestamp",
            "level",
            "category",
            "actionId",
            "adapterId",
            "physicalCode",
            "detail",
        ] {
            assert!(
                json.contains(&format!("\"{allowed}\"")),
                "serialized entry must include the `{allowed}` field"
            );
        }
        for forbidden in ["\"message\"", "\"path\"", "\"text\"", "\"command\""] {
            assert!(
                !json.contains(forbidden),
                "a persistent log must never serialize a free-text field"
            );
        }
        assert!(json.contains("\"kind\":\"execution_failed\""));
        assert!(json.contains("\"exitCode\":1"));
    }

    #[test]
    fn structured_details_cover_each_lifecycle_surface() {
        let details = [
            EventDetail::ExecutionStarted,
            EventDetail::ExecutionSucceeded,
            EventDetail::ExecutionFailed { exit_code: Some(3) },
            EventDetail::ExecutionCancelled,
            EventDetail::ExecutionTimedOut,
            EventDetail::ApprovalRequired {
                review_id: "review-1".into(),
            },
            EventDetail::ApprovalGranted {
                review_id: "review-1".into(),
            },
            EventDetail::ApprovalDenied {
                review_id: "review-1".into(),
            },
            EventDetail::CapturePaused { cancelled: 1 },
            EventDetail::CaptureResumed,
            EventDetail::Shutdown { cancelled: 2 },
            EventDetail::PermissionLost,
            EventDetail::TapDisabled {
                reason: TapDisableReason::SecureInput,
            },
            EventDetail::TapRecovered,
        ];
        for detail in details {
            let kind = detail.kind();
            assert!(!kind.is_empty());
            let json = serialize_detail(&detail);
            assert!(json.contains(&format!("\"kind\":\"{kind}\"")));
        }
    }

    #[test]
    fn file_sink_serializes_structured_json_lines() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("hotwire.log");
        let mut log = SafetyLog::file(&path).expect("file log");
        log.info(
            LogCategory::Recovery,
            EventDetail::Shutdown { cancelled: 0 },
        )
        .expect("writes");

        let contents = std::fs::read_to_string(&path).expect("read back");
        assert!(contents.contains("\"category\":\"recovery\""));
        assert!(contents.contains("\"kind\":\"shutdown\""));
        assert!(contents.trim_end().ends_with('}'));
    }
}
