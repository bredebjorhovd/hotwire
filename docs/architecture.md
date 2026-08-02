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

## Initial vertical slice

The first useful proof captures and suppresses `Numpad5`, routes
`OPEN_HERDR`, launches or focuses Herdr, and publishes an `ActionReceipt` for
the live board. A second hold route maps `Numpad0` down/up to Papegøye's
push-to-talk shortcut without repeats or stuck keys.

The checked-in Rust and TypeScript models are deliberately platform-neutral.
The macOS Quartz event-tap implementation and Tauri IPC layer will depend on
these models, not redefine them.

