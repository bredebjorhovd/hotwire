#!/usr/bin/env bash
# Runs the full Hotwire check suite: Rust fmt/clippy/tests and the pnpm
# workspace typecheck/tests/build. Mirrors what CI enforces.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "==> cargo fmt"
cargo fmt --all --check

echo "==> cargo clippy (workspace, all targets, warnings denied)"
cargo clippy --workspace --all-targets -- -D warnings

echo "==> cargo test (workspace)"
cargo test --workspace

echo "==> pnpm typecheck"
pnpm typecheck

echo "==> pnpm test"
pnpm test

echo "==> pnpm build"
pnpm build

echo "All checks passed."
