/**
 * Node-only loader for the canonical fixture profiles in `./fixtures/`.
 *
 * Used by tests (and dev tooling). Browser/Tauri runtime loading happens
 * through a future filesystem adapter, not this module.
 */

import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { parseProfileYaml, type ProfileParseResult } from "./index";

const fixturesDir = path.join(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
  "fixtures",
);

/** Names of the canonical YAML fixtures, sorted. */
export function listFixtures(): string[] {
  return readdirSync(fixturesDir)
    .filter((file) => file.endsWith(".yaml"))
    .sort();
}

/** Reads a fixture's raw YAML. */
export function loadFixture(name: string): string {
  return readFileSync(path.join(fixturesDir, name), "utf8");
}

/** Loads and validates a fixture against the canonical schema. */
export function loadFixtureProfile(name: string): ProfileParseResult {
  return parseProfileYaml(loadFixture(name));
}
