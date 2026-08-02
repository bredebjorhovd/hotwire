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
| `hotwire-input` | trigger state machine, `InputBackend` seam | → core |
| `hotwire-input-macos` | Quartz event-tap placeholder (INP-001) | → input |
| `hotwire-input-windows` | `WH_KEYBOARD_LL` placeholder (later) | → input |
| `hotwire-profile` | `Profile`/`Binding` model, YAML/JSON validation | → core |
| `hotwire-runner` | `CommandSpec` review, `CancellationToken`, timeouts | — |
| `hotwire-adapter-sdk` | `AdapterManifest`, `ActionInvocation`, `ActionResult`, `Adapter` trait | → core |
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
platforms feed it and collect results. `InputBackend` is the seam the macOS
and Windows crates implement.

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
`consumeOriginal`, canonical physical-code casing). Shell/script bindings are
reviewed against their exact command (`hotwire-runner::CommandSpec::describe`)
before first execution.

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
