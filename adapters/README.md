# adapters

First-party adapters (spec §13), one package per integration.

Adapters implement the execution contract in
`crates/hotwire-adapter-sdk` (Rust) and mirror it in `packages/schema`
(TypeScript). Each adapter owns its detection, config validation, and
execution for exactly one integration. Implementations land with ADP-001; this
directory currently documents the planned ownership:

| Directory        | Integration        | v0.1 scope                                    |
| ---------------- | ------------------ | --------------------------------------------- |
| `application`    | macOS app / bundle | launch, focus, toggle, launch-or-focus        |
| `shortcut`       | OS shortcuts       | send shortcut, mark injected events           |
| `shell`          | terminal commands  | review-before-execute, visible terminal       |
| `script`         | local scripts      | run a file path, never embedded executables   |
| `papegoye`       | Papegøye           | push-to-talk hold (shortcut), later local API |
| `herdr`          | Herdr              | launch/focus, later local API capability list |
| `claude-code`    | Claude Code        | launch + prompt handoff in configured terminal|
| `codex`          | Codex              | launch + prompt handoff, degrade to shell/app |

Hotwire never hard-codes private APIs: capability detection decides whether a
local integration path is available, and every adapter can degrade to generic
shell or application behavior.
