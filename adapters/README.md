# adapters

First-party adapters (spec §13), one package per integration.

Adapters implement the execution contract in
`crates/hotwire-adapter-sdk` (Rust) and mirror it in `packages/schema`
(TypeScript). Each adapter owns its detection, config validation, and
execution for exactly one integration.

| Directory        | Integration        | v0.1 scope                                    | Status        |
| ---------------- | ------------------ | --------------------------------------------- | ------------- |
| `application`    | macOS app / bundle | launch, focus, toggle, launch-or-focus        | planned       |
| `shortcut`       | OS shortcuts       | send shortcut, mark injected events           | planned       |
| `shell`          | terminal commands  | review-before-execute, visible terminal       | planned       |
| `script`         | local scripts      | run a file path, never embedded executables   | planned       |
| `papegoye`       | Papegøye           | push-to-talk hold (shortcut), later local API | implemented   |
| `herdr`          | Herdr              | launch/focus, later local API capability list | implemented   |
| `claude-code`    | Claude Code        | launch + prompt handoff in configured terminal| planned       |
| `codex`          | Codex              | launch + prompt handoff, degrade to shell/app | planned       |

## Implemented with ADP-001

- **`herdr/`** (`hotwire-adapter-herdr`) — negotiates Herdr capabilities and
  falls back through local API / deep link → app launch/focus → configured
  shortcut. Detection is explicit (`negotiate`), the API is never assumed
  until a capability probe succeeds, and every tier is attempted in order with
  the failures surfaced in the execution receipt. All OS interaction sits
  behind `HerdrPlatform`, so mocked integration tests cover detection, the
  fallback chain, and validation.
- **`papegøye/`** (`hotwire-adapter-papegoye`) — reproduces Papegøye's
  push-to-talk shortcut as a true hold: physical down posts the shortcut down
  once, physical up posts it up once. The hold state machine is
  reference-counted per keycode, so overlapping executions and cancellation
  during shutdown never repeat a down or leave a key stuck. No microphone data
  is part of Hotwire; Papegøye owns all audio.

Both adapters are registered in the desktop shell (`AdapterState`), which
exposes them over typed IPC (`run_adapter_action`, `release_adapter_action`,
`cancel_adapter_action`, `detect_adapter`, `validate_adapter_config`) so real
execution receipts reach the live board — the `RUN SLICE` header button drives
the spec §24 vertical slice.

Hotwire never hard-codes private APIs: capability detection decides whether a
local integration path is available, and every adapter can degrade to generic
shell or application behavior.
