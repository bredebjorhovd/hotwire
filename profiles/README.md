# profiles

User-facing profile storage.

Canonical example profiles (validated in CI) live in
`packages/profiles/fixtures/`:

- `ai-numpad.yaml` — the first-party AI Numpad layout (Herdr, Voice, Continue)
- `herdr-numpad.yaml` — minimal Herdr launch profile
- `blank.yaml` — an empty starting profile

Profiles are versioned, human-readable YAML (spec §14). Imported profiles must
pass validation before activation; the validator lives in
`packages/profiles` (TypeScript) and `crates/hotwire-profile` (Rust). The
runtime profile directory is resolved by the desktop app at startup and is
*not* this repository folder.
