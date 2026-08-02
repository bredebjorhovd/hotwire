# Safety foundation

SAFE-001 turns the safety sections of the specification (§13.3–13.4, §15, §20,
§21) into the boundaries that make execution, logging, and recovery safe. It
builds on the platform-neutral runtime (CORE-001) and the shell (APP-001).

## Command execution (`hotwire-runner`)

Commands are argument arrays, never shell strings. `CommandSpec::new(["open",
"Herdr.app", "--wait"])` carries `argv`, a working-directory strategy, a
sanitized environment, a timeout, a visible-terminal flag, and an `imported`
flag. Before anything runs — review or execution — the spec is resolved into an
immutable [`ResolvedPlan`](crate::command::ResolvedPlan): the exact working
directory and the complete child-environment snapshot, so approval and
execution operate on the same resolved plan.

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
`build_redacted()` and the env's `redactor()` (seeded from the **resolved**
environment, including inherited values) can mask their values everywhere they
might leak.

### Visible terminals (spec §13.3)

The visible-terminal path builds a POSIX script that `cd`s into the resolved
working directory, then runs `exec env -i` with only the resolved sanitized
environment and the argument array. Every word is single-quoted with correct
POSIX argv quoting, so spaces, quotes, `$VAR`, `$()`, and newlines inside an
argument are literal and cannot expand or inject; the AppleScript string handed
to `osascript` is encoded with correct AppleScript escaping, and the runner
waits for osascript's exit so a quoting or launch failure surfaces as an error
instead of a false "spawned" report.

### Timeouts and cancellation (spec §20)

`CommandRunner::run` spawns the process with `kill_on_drop` into its **own
process group**, captures stdout and stderr up to a cap (draining fully past
the cap so a high-output child never hits EPIPE), and races completion against
the spec timeout and a shared `CancellationToken`. On timeout or cancellation
the *whole group* is killed and reaped — the command and any descendants it
spawned — not just the immediate child. The visible-terminal path hands the
command to a real terminal session (`osascript` on macOS) and returns
`SpawnedInTerminal`; the default for user-authored development commands is a
visible terminal (spec §13.3). Visible-terminal runs are explicitly
**untracked**: the runner does not wait on them and claims no timeout or
cancellation coverage.

## Risk classification (spec §15.2)

`classify_argv` maps every command to `RiskLevel`, conservatively:

- **Confirmation** — destructive programs (`rm`, `dd`, `mkfs`, `diskutil`,
  `shutdown`, `chmod`, …), destructive argument forms on approved CLIs
  (`git clean -fdx`, `git reset --hard`, `git push --force`, `cp -f`,
  `mv --force`), destructive payloads passed to shell interpreters
  (`sh`/`bash`/`zsh` `-c "rm -rf …"`, even user-authored), and arbitrary
  executables from imported profiles (anything not on the approved-CLI list).
  Unknown imported forms fail toward confirmation.
- **Low** — approved CLIs (`open`, `git status`, `herdr`, `claude`, …) and
  non-destructive user-authored commands.

## Review-before-execute (spec §14, §15.2)

`ApprovalStore` implements imported-command approval, and `CommandRunner`
**enforces it on the only public execution path**: a confirmation-risk imported
command is first resolved into an immutable [`ResolvedPlan`] (working directory
and complete environment snapshot), returns `RunStatus::ApprovalRequired`
without starting anything until that exact plan is approved, and then executes
precisely the approved plan. The first execution produces a `PendingReview`
carrying the exact command line (`ResolvedPlan::describe`); `approve` lets it
run and `deny` drops it without approving. Because approval is bound to the
**resolved plan** — argv, the exact working directory, the full environment
snapshot (including inherited values), timeout, terminal mode, and provenance —
running in a different project directory or after an inherited `PATH` changes
yields a different plan that must be reviewed again.

## Redacted, structured local logs (spec §15.1, §15.3)

`LogEntry` has a closed field set — timestamp, level, category, and validated
identifier newtypes (`ActionId`, `AdapterId`, `PhysicalCode`, `ReviewId`) plus
a structured `EventDetail` (success/failure with an exit code, approval
lifecycle, capture pause/resume, shutdown, tap health). There is **no
free-text field at all**, and every string-bearing field is a validated
identifier (prompts, paths, and key sequences are rejected at construction),
so forbidden payloads are *unrepresentable* in the persistent log.
`SafetyLog` writes only these allowlisted fields to `InMemorySink` or
`FileSink` (JSON lines). Raw-event capture is a **separate, explicit opt-in
surface** that auto-disables after a short window capped at 60 s, keeps a
bounded ring buffer of 1024 samples, and is never persisted
(`RawEventDiagnostics`).

Secret hygiene is layered: `SanitizedEnv` tracks marked keys, redacts them from
the built environment, and seeds a `Redactor` from the **resolved**
environment (explicit *and* inherited values), so a secret the child echoes
bare is masked. Derived `Debug` output for `SanitizedEnv`, `CommandSpec`, and
`ResolvedPlan` masks secret values and runs argv through the env's redactor.
Visible-terminal runs that carry marked secrets are **refused** rather than
rendering the secrets into the terminal command line or scrollback.

The structural guarantee is that **diagnostics never contain typed text,
prompts, secrets, or arbitrary key sequences**: the log model has no field for
them, and Debug/redaction output is masked. Logs are local by construction —
there is no network code here.

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
