# Development guide

## Requirements

- Node.js 22+ and pnpm 10+
- Stable Rust (1.81+)
- macOS for the Tauri desktop shell

## One-command check

`./scripts/check.sh` runs the whole suite that CI enforces:

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace`
4. `pnpm typecheck`
5. `pnpm test`
6. `pnpm build`

## The desktop shell

The Tauri shell lives in `apps/desktop/src-tauri` and is a normal member of
the Rust workspace, so `cargo test --workspace` compiles it. A full CI-quality
pass compiles the whole stack including the `tauri` crate.

```sh
# Interactive development: vite + tauri webview
pnpm tauri dev

# Frontend-only preview (no Rust shell; IPC degrades gracefully)
pnpm dev
```

The shell's `tauri.conf.json` points `frontendDist` at `../dist`, so run
`pnpm build` once before bundling. Dev uses `devUrl` `http://localhost:1420`.
`cargo build`/`cargo test` work without the frontend build; `pnpm tauri build`
needs it.

### Regenerating icons

`scripts/gen-icon.py` draws `apps/desktop/src-tauri/icons/icon.png`. For a full
platform icon set, run:

```sh
cd apps/desktop
pnpm tauri icon src-tauri/icons/icon.png
```

## Adding a crate or package

- A new Rust crate: add `crates/<name>/` and append it to `members` in the
  root `Cargo.toml`. Reuse shared deps through `[workspace.dependencies]`.
- A new pnpm package: add `packages/<name>/` (it is picked up by
  `pnpm-workspace.yaml` automatically).

## Style

- Rust: `rustfmt` + `clippy` with the workspace lints (`pedantic` on, `unsafe`
  denied). CI denies all warnings.
- TypeScript: strict `tsc`, Zod schemas as the single validation source.
- New behavior ships with a test in the owning crate/package.
