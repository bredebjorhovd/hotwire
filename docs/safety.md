# Safety foundation

SAFE-001 turns the safety sections of the specification (§13.3–13.4, §15, §20,
§21) into the boundaries that make execution, logging, and recovery safe. It
builds on the platform-neutral runtime (CORE-001) and the shell (APP-001).

## Command execution (`hotwire-runner`)

Commands are argument arrays, never shell strings. `CommandSpec::new(["open",
"Herdr.app", "--wait"])` carries `argv`, a working-directory strategy, a
sanitized environment, a timeout, a visible-terminal flag, and an `imported`
flag.

### Working-directory strategies (spec §13.3)

`CwdStrategy` resolves where a command runs:

- `Fixed(path)` — always that directory.
- `Home` — the user's home directory.
- `CurrentProject` — the current project from a configured IDE integration,
  supplied as a run-time hint.
- `Ask` — defer to the user; the runner refuses to pick a directory.

The runner resolves the strategy before spawning; an unresolvable directory is
a clean `StartError`, never a guess.

### Sanitized environments (spec §15.3)

`SanitizedEnv` rebuilds the child environment from scratch: the host
environment is cleared, an explicit allowlist of variables is carried over, and
explicit variables win. Secret keys are tracked separately, so
`build_redacted()` and the env's `redactor()` can mask their values everywhere
they might leak.

### Timeouts and cancellation (spec §20)

`CommandRunner::run` spawns the process with `kill_on_drop`, captures stdout and
stderr up to a cap, and races completion against the spec timeout and a shared
`CancellationToken`. Either one kills the child. The visible-terminal path hands
the command to a real terminal session (`osascript` on macOS) and returns
`SpawnedInTerminal`; the default for user-authored development commands is a
visible terminal (spec §13.3). Background commands are the tracked path.

## Risk classification (spec §15.2)

`classify_command_risk` maps every command to `RiskLevel`:

- **Confirmation** — destructive programs (`rm`, `dd`, `mkfs`, `diskutil`,
  `shutdown`, …) and arbitrary executables from imported profiles (anything
  not on the approved-CLI list).
- **Low** — approved CLIs (`open`, `git`, `herdr`, `claude`, …) and
  non-destructive user-authored commands.

## Review-before-execute (spec §14, §15.2)

`ApprovalStore` implements imported-command approval. The first execution of an
imported confirmation-level action produces a `PendingReview` carrying the
exact command line (`CommandSpec::describe`); it runs only after `approve`,
and the same command line stays approved for later presses. `deny` drops the
review without approving anything.

## Redacted local logs (spec §15.1, §15.3)

`LogEntry` has a closed field set: timestamp, level, category, outcome, action
id, adapter id, and the single matched physical code — plus a free-text
`message`. `SafetyLog` pushes every message through a `Redactor` before the
sink sees it, so secret values and `KEY=value` tokens (like
`ANTHROPIC_API_KEY=…`) are masked even when the exact value was not registered.
`InMemorySink` and `FileSink` (JSON lines) are provided.

The structural guarantee is that **diagnostics never contain typed text,
prompts, secrets, or arbitrary key sequences**: the log model has no field for
them, and the message field is redacted. Logs are local by construction — there
is no network code here.

## Diagnostics and recovery

### Capture health

`hotwire-core::CaptureHealth` (permission, tap status, paused) is the neutral
model every backend reports. `CaptureHealth::fail_open` is true when the
process lost its input permission, the tap is stopped or failed, secure input
disabled it, or capture is paused — and `CaptureGate::decide_with_health` never
suppresses a key when health fails open. `QuartzEventTap::health()` fills the
snapshot from the live tap.

### Pause / resume / shutdown

`HotwireRuntime::pause` stops routing, resets the router's interaction state
(so a held key that was cancelled does not linger), and cancels every in-flight
execution. `resume` restores routing. `shutdown` pauses permanently and cancels
everything — the clean-shutdown surface (spec §15.5): after it returns, no
action can fire and no key is left held.

The shell exposes these as `pause_capture` / `resume_capture` IPC commands and a
menu-bar "Pause capture" item, and `diagnostics` returns a
`DiagnosticsReport` whose fields are limited to permitted categories.

## Telemetry (spec §21)

Telemetry is **off by default**. `TelemetryPolicy::default().enabled == false`.
Even the optional opt-in categories are restricted (app version, OS version,
permission-failure category, execution success/failure category, crash
reports); the diagnostics and log models cannot carry key sequences, commands,
prompts, file paths, application titles, clipboard, or selected text.
