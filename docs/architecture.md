# Architecture foundation

Hotwire is split at the point where timing and safety requirements change.

```text
native input callback → normalized event queue → binding router
  → semantic action → adapter → execution receipt → live board UI
```

## Non-negotiable invariants

1. Native input callbacks normalize and enqueue only; they never execute an action.
2. Unmatched input passes through unchanged.
3. Injected events are tagged and ignored by the interceptor.
4. Losing permissions or crashing restores normal keyboard behavior.
5. Profiles are versioned, human-readable, and validated before activation.
6. Imported shell and script actions expose exact commands before first execution.
7. General key events, typed text, prompts, and secrets are never logged.

## Crate and package ownership

The workspace is layered so that no higher layer may redefine a lower one.

| Component | Owns | Dependency direction |
| --- | --- | --- |
| `hotwire-core` | `PhysicalKeyEvent`, `KeyState`, `ModifierState`, `Trigger`, `ActionStatus`, `ActionReceipt`, `CaptureHealth`, `DiagnosticsReport`, `TelemetryPolicy` | — |
| `hotwire-input` | trigger state machine, `CaptureGate`/`EmergencyBypass`, `InputBackend` seam, fail-open health gate | → core |
| `hotwire-input-macos` | Quartz event-tap backend (capture, suppression, injection, fail-open, `health()` diagnostics) | → input |
| `hotwire-input-windows` | `WH_KEYBOARD_LL` placeholder (later) | → input |
| `hotwire-profile` | `Profile`/`Binding` model, `CaptureMode`, YAML/JSON validation + export | → core |
| `hotwire-runner` | `CommandSpec` argv/cwd/env/timeout, risk classification, review-before-execute, `CommandRunner`, redacted logs | — |
| `hotwire-adapter-sdk` | `AdapterManifest`, `ActionInvocation`, `ActionResult`, `Adapter` trait | → core |
| `hotwire-router` | `BindingRouter`, `AdapterRegistry`, `HotwireRuntime` (pause/resume/shutdown), receipts | → core, input, profile, adapter-sdk |
| `apps/desktop/src-tauri` | Tauri shell, typed IPC commands, diagnostics/pause/recovery surfaces, menu bar | → core, profile, adapter-sdk, input-macos |
| `packages/schema` | Zod boundary types (profile, action, adapter, execution) | — |
| `packages/profiles` | YAML parse/export, canonical fixtures | → schema |
| `apps/desktop/src` | React prototype, typed IPC bridge | → schema, profiles |

Platform backends only hand normalized events to the core; they never run
actions. Adapters only execute; they never interpret input. The Tauri shell
only exposes boundaries over IPC.

## Typed boundaries

### Normalized physical-key events (`hotwire-core`)

`PhysicalKeyEvent` is the single contract between native capture and the
binding router. Backends normalize raw OS events into it (scan code, physical
code, state, modifiers, timestamp, repeat/injected flags) and enqueue them;
the router never sees raw OS structures. `should_route` rejects injected and
repeat events before binding lookup to prevent recursion.

### Trigger detection (`hotwire-input`)

`TriggerDetector` turns a `PhysicalKeyEvent` stream into `TriggerEvent`s
(down/up/cancelled) for `press`, `hold`, and `double_press`. It is pure —
platforms feed it and collect results. `CaptureGate` combines `CapturePolicy`
(mode + bound keys) with `EmergencyBypass` (⌃⌥⌘Esc) into the single
suppression decision the native callback makes; both are unit-tested without
any OS calls. `InputBackend` is the seam the macOS and Windows crates
implement, with `stop()` releasing resources fail-open.

### Quartz event tap (`hotwire-input-macos`)

The macOS proof (INP-001) runs a `CGEventTap` on a dedicated thread. The
callback normalizes each key event to `PhysicalKeyEvent`, passes Hotwire's own
injected events (tagged with `INJECTED_MARKER` in the event user-data field)
straight through, and returns `Drop` only for bound keys while capture is
active. It routes events to the action sink except while capture is paused and
except for the emergency-bypass chord itself (which can never be remapped);
the decision/observability channel always receives every event. The tap
re-enables itself after `TapDisabledByTimeout`, goes fail-open on
`TapDisabledByUserInput` and on any permission loss, and `stop()` releases
every logically held key so shutdown can never leave one down. See
`docs/input-proof.md` for setup and manual verification.

### Binding routing and execution (`hotwire-router`)

`BindingRouter` is the pure state machine that turns events into decisions. It
holds one `TriggerDetector` per enabled binding, groups detectors by physical
code, and for every event decides:

- **Firing** — which binding won the interaction, preferring a layer binding
  (`layer: true`) over a normal one on the same key while the layer key is
  held. Disabled bindings are dropped at construction and never fire.
- **Layer gating** — the profile's single `layerKey` (spec §9.2) flips on
  key-down and off on key-up; layer bindings only fire while it is held.
- **Capture modes** (spec §9.3) — `capture` consumes per
  `consumeOriginal`, `modified_capture` only captures while the layer key is
  held, and `passthrough` observes without ever consuming.
- **Consumption** — `consumeOriginal` applies to the down that fired and its
  matching up (and to repeat downs while a hold is active), so a held key
  never re-fires and never leaves a key logically held down. For a
  `double_press`, consumption stays armed from the first press through its
  key-up, so a mapped numpad digit never reaches the foreground app while the
  second press is still expected.
- **Disabled profiles** — `BindingRouter::new` rejects `Profile { enabled:
  false }` with `RouterError::ProfileDisabled`; enabling a profile means
  constructing a router for it, so a disabled profile can never produce an
  action.
- **Hold lifecycle** — `hold` fires once on down and releases exactly once on
  up (`RouteOutcome::releases`), so push-to-talk starts and stops cleanly.

The router is pure: it never touches the OS and never awaits an adapter.
`AdapterRegistry` holds registered adapters by manifest id and is the only path
to an adapter (execution, release, cancellation). `HotwireRuntime` composes
them — it executes fired actions, ends and cancels holds, tracks in-flight
executions by id, and broadcasts every `ActionReceipt`
(`started`/`succeeded`/`failed`/`cancelled`) to live-board, log, and
diagnostics subscribers. Events must be fed to the runtime from an async task,
never from a native input callback.

### Semantic actions and adapter execution (`hotwire-adapter-sdk` / `packages/schema`)

Semantic actions are stable intent names (`app.open_or_focus`, `voice.input`)
with a catalog entry (`ActionDefinition`). Adapters declare a `Manifest`,
detect availability, validate binding config, and execute an
`ActionInvocation` into an `ActionResult` (`started`/`succeeded`/`failed`/
`cancelled`) that feeds the live board and logs. Rust and TypeScript model the
same shapes (`camelCase`), so `ActionInvocation` and `ActionResult` can cross
IPC verbatim.

### Profile validation (`hotwire-profile` / `@hotwire/schema`)

Profiles are YAML documents validated against schema version 1 before
activation. `hotwire-profile::parse_yaml` (Rust) and
`@hotwire/profiles::parseProfileYaml` (TypeScript) validate the same document
shape and normalize shorthand (generated binding ids, defaulted
`consumeOriginal`, canonical physical-code casing). Imported profiles declare
`schemaVersion: 1`; any other version is rejected before activation.
`hotwire-profile::export_yaml`/`export_json` render validated profiles back to
readable, shareable documents. Shell/script bindings are reviewed against
their exact command (`hotwire-runner::CommandSpec::describe`) before first
execution.

### Safe execution, review, and redacted logs (`hotwire-runner`)

The runner owns the review-before-execute and cancellation boundaries. A
`CommandSpec` is an argument array with a `CwdStrategy` (fixed, home, current
project, or ask — spec §13.3), a `SanitizedEnv` that rebuilds the child
environment from an allowlist plus explicit variables and tracks secret keys,
a timeout, and a visible-terminal flag (default on for development commands).
`classify_command_risk` marks destructive programs and arbitrary imported
executables as confirmation-risk. `CommandRunner` is the *only* public
execution path and **enforces** review-before-execute: `ApprovalStore` requires
the exact structured spec (not just a rendered string) to be approved before an
imported confirmation-risk command's first run (spec §15.2), and refuses with
`RunStatus::ApprovalRequired` otherwise. Background commands run in their own
process group, so a timeout or cancellation kills and reaps the whole tree; the
visible-terminal path (correct POSIX argv quoting, AppleScript encoding, and
`env -i` with only the sanitized environment) is explicitly untracked.
`SafetyLog` writes a closed-field `LogEntry` with a structured `EventDetail` —
there is no free-text field, so typed text, prompts, paths, and key sequences
are unrepresentable (spec §15.1/§15.3); raw-event diagnostics are a separate
opt-in, auto-expiring, never-persisted surface. Derived `Debug` output masks
secret values. See `docs/safety.md`.

### Diagnostics and recovery

`hotwire-core` owns the neutral diagnostics model: `CaptureHealth`
(permission, tap status, paused), `ActionSummary`, `DiagnosticsReport`, and
`TelemetryPolicy` (off by default, spec §21). `CaptureHealth::fail_open` is the
single fail-open decision; `CaptureGate::decide_with_health` never suppresses a
key when capture is unhealthy (spec §15.5). `HotwireRuntime` adds pause/resume
(reset the router, cancel in-flight executions, no key left held) and a
permanent `shutdown`. The shell exposes `diagnostics`, `pause_capture`,
`resume_capture` over IPC and a menu-bar "Pause capture" item, so recovery is
available without opening the main window. Diagnostics and logs never contain
typed text, prompts, secrets, or arbitrary key sequences.

## IPC surface (`apps/desktop/src-tauri`)

The Tauri shell registers a small typed surface in `commands.rs` and one
event in `events.rs`:

Commands:

- `app_status` → shell version, profile schema version, input backend state
- `validate_profile(yaml)` → validated profile or a readable error
- `show_main_window` → reveals and focuses the configuration window
- `quit` → exits the shell cleanly
- `mock_action_receipt` → emits a mocked `ActionReceipt` event (native capture
  is INP-001; this exercises the event path with the real `hotwire-core`
  payload shape)
- `diagnostics` → a `DiagnosticsReport` snapshot (capture health, app version,
  last action summary) restricted to permitted categories (spec §6.4, §21)
- `pause_capture` / `resume_capture` → toggle the fail-open capture pause and
  re-label the menu-bar item, returning the new paused state

Event:

- `action-receipt` (payload `hotwire-core::ActionReceipt`, camelCase) →
  broadcast to every webview so the live board, logs, and the menu-bar
  popover's "last action" readout can react.

The frontend bridge (`apps/desktop/src/features/bridge/ipc.ts`) wraps the
commands and `subscribeActionReceipts` wraps the event, each with fallbacks
so the plain-vite prototype still runs without the shell. `mock_action_receipt`
and its event are the typed boundary the UI is developed against until native
capture lands.

## Menu-bar lifecycle (`apps/desktop/src-tauri`)

Hotwire is a menu-bar app (spec §6.1). `tray.rs` owns a tray icon whose menu
provides "Open Hotwire…" (reveals the configuration window), a "Pause capture"
item that toggles the fail-open pause on the shared tap (a recovery surface
available without opening the window), and "Quit". `window.rs` owns the
configuration-window lifecycle: the red close button hides the window instead
of quitting, so the app keeps running in the menu bar; `quit` and the
Dock-reopen event (`RunEvent::Reopen`) re-show it.

## Initial vertical slice

The first useful proof captures and suppresses `Numpad5`, routes
`OPEN_HERDR`, launches or focuses Herdr, and publishes an `ActionReceipt` for
the live board. A second hold route maps `Numpad0` down/up to Papegøye's
push-to-talk shortcut without repeats or stuck keys.

The checked-in Rust and TypeScript models are deliberately platform-neutral.
The macOS Quartz event-tap implementation and Tauri IPC layer will depend on
these models, not redefine them.
