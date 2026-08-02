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
| `hotwire-core` | `PhysicalKeyEvent`, `KeyState`, `ModifierState`, `Trigger`, `ActionStatus`, `ActionReceipt` | — |
| `hotwire-input` | trigger state machine, `CaptureGate`/`EmergencyBypass`, `InputBackend` seam | → core |
| `hotwire-input-macos` | Quartz event-tap backend (capture, suppression, injection, fail-open) | → input |
| `hotwire-input-windows` | `WH_KEYBOARD_LL` placeholder (later) | → input |
| `hotwire-profile` | `Profile`/`Binding` model, `CaptureMode`, YAML/JSON validation + export | → core |
| `hotwire-runner` | `CommandSpec` review, `CancellationToken`, timeouts | — |
| `hotwire-adapter-sdk` | `AdapterManifest`, `ActionInvocation`, `ActionResult`, `Adapter` trait | → core |
| `hotwire-router` | `BindingRouter`, `AdapterRegistry`, `HotwireRuntime`, receipts | → core, input, profile, adapter-sdk |
| `apps/desktop/src-tauri` | Tauri shell, typed IPC commands | → core, profile, adapter-sdk |
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

## IPC surface (`apps/desktop/src-tauri`)

The Tauri shell registers a small typed surface in `commands.rs`:

- `app_status` → shell version, profile schema version, input backend state
- `validate_profile(yaml)` → validated profile or a readable error

The frontend bridge (`apps/desktop/src/features/bridge/ipc.ts`) wraps these
with fallbacks so the plain-vite prototype still runs without the shell.

## Initial vertical slice

The first useful proof captures and suppresses `Numpad5`, routes
`OPEN_HERDR`, launches or focuses Herdr, and publishes an `ActionReceipt` for
the live board. A second hold route maps `Numpad0` down/up to Papegøye's
push-to-talk shortcut without repeats or stuck keys.

The checked-in Rust and TypeScript models are deliberately platform-neutral.
The macOS Quartz event-tap implementation and Tauri IPC layer will depend on
these models, not redefine them.
