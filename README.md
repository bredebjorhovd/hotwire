# Hotwire

Turn unused keys into a control surface for your tools.

Hotwire is a local-first desktop utility that maps physical keys to semantic
actions for Herdr, Claude Code, Codex, Papegøye, applications, shortcuts, and
developer commands.

The first implementation slice targets macOS and proves two routes:

- `Numpad5 → OPEN_HERDR → launch or focus Herdr`
- `Numpad0 hold → VOICE → hold Papegøye push-to-talk`

## Repository layout

- `apps/desktop` — React interaction prototype and future Tauri shell
- `crates/hotwire-core` — normalized input and action-routing domain model
- `packages/schema` — shared, versioned TypeScript profile schema
- `docs/architecture.md` — implementation boundaries and safety invariants

## Development

Prerequisites: Node.js 22+, pnpm 10+, and stable Rust.

```sh
pnpm install
pnpm test
pnpm build
cargo test --workspace
```

Hotwire is pre-alpha. The current code is the build foundation for the product
specification and does not yet intercept system keyboard events.

## License

Apache-2.0. See `LICENSE`.

