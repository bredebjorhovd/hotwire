/**
 * Browser-safe loader for the canonical profile fixtures.
 *
 * The `@hotwire/profiles` fixtures module is Node-only (`readFileSync`), so the
 * prototype imports the same YAML documents as raw strings and validates them
 * through the shared `parseProfileYaml` boundary. This keeps the demo strictly
 * fixture-driven with no runtime filesystem access.
 */

import { parseProfileYaml, type ProfileParseResult } from "@hotwire/profiles";
import type { Profile } from "@hotwire/schema";

import aiNumpadYaml from "../../../../../packages/profiles/fixtures/ai-numpad.yaml?raw";
import blankYaml from "../../../../../packages/profiles/fixtures/blank.yaml?raw";
import claudeNumpadYaml from "../../../../../packages/profiles/fixtures/claude-numpad.yaml?raw";
import cometNumpadYaml from "../../../../../packages/profiles/fixtures/comet-numpad.yaml?raw";
import codexNumpadYaml from "../../../../../packages/profiles/fixtures/codex-numpad.yaml?raw";
import herdrNumpadYaml from "../../../../../packages/profiles/fixtures/herdr-numpad.yaml?raw";
import voiceAppsNumpadYaml from "../../../../../packages/profiles/fixtures/voice-apps-numpad.yaml?raw";

const raw: Array<{ id: string; yaml: string }> = [
  { id: "ai-numpad", yaml: aiNumpadYaml },
  { id: "blank", yaml: blankYaml },
  { id: "claude-numpad", yaml: claudeNumpadYaml },
  { id: "comet-numpad", yaml: cometNumpadYaml },
  { id: "codex-numpad", yaml: codexNumpadYaml },
  { id: "herdr-numpad", yaml: herdrNumpadYaml },
  { id: "voice-apps-numpad", yaml: voiceAppsNumpadYaml },
];

const parsed = new Map<string, ProfileParseResult>(
  raw.map(({ id, yaml }) => [id, parseProfileYaml(yaml)]),
);

/**
 * Returns the parsed, schema-validated fixture profile for `id`.
 *
 * Throws only if the fixture itself is broken (a build-time invariant guarded
 * by the `@hotwire/profiles` fixture tests).
 */
export function loadFixtureProfile(id: string): Profile {
  const result = parsed.get(id);
  if (!result?.ok) {
    throw new Error(`fixture profile "${id}" failed to validate: ${result?.error ?? "unknown"}`);
  }
  return result.profile;
}

/** Returns the canonical YAML for activating a profile in the native shell. */
export function loadFixtureYaml(id: string): string {
  const fixture = raw.find((entry) => entry.id === id);
  if (!fixture) throw new Error(`unknown fixture profile "${id}"`);
  return fixture.yaml;
}

export const fixtureProfileIds = raw.map(({ id }) => id);
