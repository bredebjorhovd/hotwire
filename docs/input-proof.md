# macOS input proof (INP-001)

Milestone 1 proves the hardest OS integration: capturing and suppressing
selected numpad keys through a Quartz event tap while passing everything else
through, without ever breaking the keyboard.

## What is implemented

`hotwire-input-macos` provides a real `CGEventTap` backend:

- **Capture** — bind physical codes (`Numpad5`, `Numpad0`, …); while capture is
  active their key-down and key-up events are consumed and never reach other
  applications. Unbound keys and all keys in `Passthrough` mode pass through
  unchanged.
- **Normalization** — every key event is converted to the shared
  `hotwire-core::PhysicalKeyEvent` (physical code, scan code, state,
  modifiers, ns timestamp, repeat/injected flags) inside the callback and
  enqueued on a channel. `CGEventTimestamp` is already elapsed nanoseconds, so
  it is copied verbatim — never scaled. The callback never executes actions
  (spec §10.3).
- **Injection-loop prevention** — Hotwire's own injected events carry a marker
  in the event user-data field and are always passed through untouched.
- **Emergency bypass** — `Control` + `Option` + `Command` + `Escape` pauses and
  resumes capture. It cannot be remapped by a profile: the chord event itself
  is never routed as a binding, and while capture is paused no events are
  routed to the action sink at all (the decision/observability channel keeps
  receiving them). Ordinary `Passthrough` mode still routes events for
  observation without suppressing them.
- **Fail-open** — if capture is paused, the tap is disabled, or the process
  crashes, every key passes through normally. No key is ever held logically:
  `stop()` (and shutdown) posts key-ups for any key Hotwire still holds.
- **Recovery** — `TapDisabledByTimeout` re-enables the tap automatically;
  `TapDisabledByUserInput` (secure input) is respected and surfaced as
  `TapStatus::DisabledByUserInput` so the app can warn the user.

Platform-neutral decision logic lives in `hotwire-input`
(`CapturePolicy`, `CaptureGate`, `EmergencyBypass`) and is fully unit-tested
without any OS calls.

## Granting permission

Hotwire needs the **Accessibility** permission (event taps and event posting
both require it).

1. Open **System Settings → Privacy & Security → Accessibility**.
2. Add the terminal (or the Hotwire app) that runs the probe.
3. Re-run. The probe reports `permission: authorized` when it is granted.

The permission can be revoked at any time; Hotwire then degrades to fail-open
(passthrough) rather than intercepting.

## Running the unit tests

```sh
cargo test -p hotwire-input
cargo test -p hotwire-input-macos
```

These run in CI on `macos-latest` and need no permission.

## Manual verification with the probe

The probe is an interactive harness. It refuses to run unless
`HOTWIRE_PROBE=1` is set, because starting it really does consume keys.

```sh
HOTWIRE_PROBE=1 cargo run -p hotwire-input-macos --example probe
```

It binds `Numpad5` and `Numpad0` by default (pass other physical-code names as
arguments to bind different keys). Then follow the printed instructions:

| Action | Expected output |
| --- | --- |
| Press `Num 5` | `[SUPPRESS] Numpad5 down` / `[SUPPRESS] Numpad5 up` — the `5` does not reach other apps |
| Press a normal key, e.g. `A` | `[PASS] A down` / `[PASS] A up` — it reaches other apps normally |
| Hold `Control`+`Option`+`Command` and press `Esc` | `[EMERGENCY] capture paused`; every key now shows `[PASS]` |
| Press the chord again | `[EMERGENCY] capture resumed`; bound keys show `[SUPPRESS]` again |
| `Ctrl+C` | Tap stops cleanly; held keys are released |

Suggested checks for a thorough pass (spec §20):

- Press `Num 5` with Num Lock **on and off** — suppression is physical-code
  based and must be identical.
- Press and hold `Num 5` — only the initial `[SUPPRESS] … down` fires; repeats
  are suppressed and do not leak.
- Pause via the emergency chord, type in a text field, then resume.
- Quit mid-hold — no key stays logically held down.

## Guarded integration test

The end-to-end test drives the real event pipeline: it starts a tap, posts
unmarked synthetic events through `CGEventPost`, and asserts the decisions. It
is `#[ignore]`d and guarded by an env var and the Accessibility check, so it
never runs in normal CI.

```sh
HOTWIRE_INTEGRATION=1 cargo test -p hotwire-input-macos --test event_tap -- --ignored
```

It verifies:

1. A bound `Numpad5` press is consumed (down and up).
2. An unbound `A` press passes through.
3. The emergency chord pauses capture (fail-open) and resumes it.
4. Shutdown releases every logically held key.

> Note: the passthrough checks post a real keystroke into the system stream,
> so the active application may receive an `a` or a space. Run it when a stray
> keystroke is harmless.

## Integration surface for APP-001

The app drives the tap through the concrete `QuartzEventTap` API:

```rust
let tap = hotwire_input_macos::QuartzEventTap::new();
let (tx, rx) = std::sync::mpsc::channel();
tap.set_capture_mode(CaptureMode::Capture);
tap.set_captured_keys(&["Numpad5".to_string(), "Numpad0".to_string()]);
tap.start(tx)?;                       // events arrive on rx
tap.emergency_pause();                // or the user presses ⌃⌥⌘Esc
tap.stop();                           // releases held keys, fail-open
```

`QuartzEventTap` also implements `hotwire_input::InputBackend`, so
platform-neutral code can start/stop it through the seam. Diagnostics live in
`permission_status()` (`PermissionStatus`) and `status()` (`TapStatus`).
Injection for the shortcut adapter is `tap.injector()` (`MacEventInjector`),
which tags events with `INJECTED_MARKER` and tracks held keys for shutdown
release.

## Threading and safety notes

- The tap runs on a dedicated thread driving a `CFRunLoop`; the callback only
  normalizes, gates, enqueues, and returns `Drop`/`Keep`.
- The only raw FFI is a small audited module (`src/ffi.rs`): `AXIsProcessTrusted`
  and `CGEventGetTimestamp`. Everything else uses the safe `core-graphics`
  wrapper, and the workspace `unsafe_code = deny` lint is relaxed only there.
- `tap.rs` re-enables the tap after `TapDisabledByTimeout`; it deliberately does
  not fight `TapDisabledByUserInput` (secure input), going fail-open instead.
