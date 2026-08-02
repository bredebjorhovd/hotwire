# Hotwire

Turn unused keys into a control surface for your tools.

Hotwire is a local-first desktop utility that maps physical keys to semantic
actions for Herdr, Claude Code, Codex, Papegøye, applications, shortcuts, and
developer commands. The first implementation slice targets macOS and proves two
routes:

- `Numpad5 → OPEN_HERDR → launch or focus Herdr`
- `Numpad0 hold → VOICE → hold Papegøye push-to-talk`

The build foundation (BOOT-001) established the Tauri 2 + React desktop
shell, a Rust workspace, shared schema and profile packages, CI, and the typed
boundaries between input, actions, adapters, and profiles. CORE-001 added the
platform-neutral runtime: the press/hold/double-press state machines, layer
and capture-mode behavior, `consumeOriginal` decisions, the adapter registry,
cancellation, `ActionReceipt` events, and readable YAML import/export. UX-001
added the Milestone 0 interaction prototype: an eight-screen first-run wizard
(welcome, control surface, permission, hardware capture, starting profile,
connected tools, live board test, done) with a tactile numpad signature object,
dark/light tokens, animated signal-trace route receipts, keyboard navigation
and reduced-motion support. The prototype is fixture-driven and renders fully
in the browser (`pnpm dev`); the Rust shell adds the typed IPC boundary.

Native capture on macOS is proven by INP-001 (`hotwire-input-macos`): a Quartz
event tap that captures and suppresses selected numpad keys, passes everything
else through, filters Hotwire's own injected events, and fails open on shutdown
or permission loss. See `docs/input-proof.md`.

## Repository layout

```text
hotwire/
├── apps/
│   └── desktop/            React interaction prototype + Tauri 2 shell
│       ├── src/            app / components / features / routes / styles
│       └── src-tauri/      Rust shell, IPC commands, capabilities
├── crates/
│   ├── hotwire-core/       normalized events, triggers, action receipts
│   ├── hotwire-input/      trigger detection + input-backend seam
│   ├── hotwire-input-macos/   Quartz event-tap proof (INP-001)
│   ├── hotwire-input-windows/ WH_KEYBOARD_LL seam (later)
│   ├── hotwire-runner/     command review + timeout/cancellation boundary
│   ├── hotwire-profile/    profile model + YAML/JSON validation + export
│   ├── hotwire-adapter-sdk/   adapter execution contract
│   └── hotwire-router/     binding router, adapter registry, runtime, receipts
├── packages/
│   ├── schema/             versioned Zod boundary types
│   └── profiles/           YAML parsing/export + canonical fixtures
├── adapters/               first-party adapter ownership (ADP-001)
├── profiles/               profile storage location
├── scripts/                check.sh, icon generator
├── docs/                   architecture + development guides
└── .github/workflows/      CI
```

## Prerequisites

- Node.js 22+
- pnpm 10+
- Stable Rust (1.81+)
- macOS for the Tauri desktop shell

## Development

```sh
# Install dependencies (frontend + workspace links)
pnpm install

# Type-check, test, and build the pnpm workspace
pnpm typecheck
pnpm test
pnpm build

# Rust workspace: format, lint, and test
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Everything above in one command
./scripts/check.sh

# Desktop shell (Tauri): dev app and frontend-only preview
pnpm tauri dev          # from apps/desktop or the repo root
pnpm dev                # plain-vite preview without the Rust shell
```

`pnpm tauri dev` runs the React frontend inside the Tauri webview; the status
bar at the bottom of the prototype proves the Rust↔TypeScript IPC boundary
(and degrades gracefully in the plain `pnpm dev` preview).

## Typed boundaries

| Boundary | Rust | TypeScript |
| --- | --- | --- |
| Normalized physical-key events | `hotwire-core::PhysicalKeyEvent` | — (native only) |
| Triggers / capture gate | `hotwire-input` | `triggerSchema` |
| Routing / layers / capture modes | `hotwire-router::BindingRouter` | `captureModeSchema` + `layer` |
| Adapter execution | `hotwire-adapter-sdk` | `actionInvocationSchema` / `actionResultSchema` |
| Adapter registry / runtime | `hotwire-router::AdapterRegistry` / `HotwireRuntime` | — (native only) |
| Execution receipts | `hotwire-core::ActionReceipt` | — (native only) |
| Profile validation + export | `hotwire-profile` | `@hotwire/schema` + `@hotwire/profiles` |

Profiles are versioned, human-readable YAML. Imported profiles must validate
before activation, and the Rust and TypeScript validators agree on the same
document shape (see `docs/architecture.md`). The native input proof and its
manual verification are described in `docs/input-proof.md`.

## License

Apache-2.0. See `LICENSE`.
