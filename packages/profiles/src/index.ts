/**
 * Profile document parsing and export.
 *
 * Profiles are human-readable YAML that must validate before activation. This
 * module parses spec-style shorthand (ids optional, `consumeOriginal`
 * optional) into the canonical `Profile` shape owned by `@hotwire/schema`, and
 * serializes canonical profiles back to readable YAML for export.
 */

import { ZodError } from "zod";
import { parse as parseYaml, stringify as stringifyYaml } from "yaml";

import {
  normalizePhysicalCode,
  profileSchema,
  type Profile,
} from "@hotwire/schema";

export type ProfileParseResult =
  | { ok: true; profile: Profile }
  | { ok: false; error: string };

/**
 * Parses and validates a profile YAML document.
 *
 * Accepts the spec's shorthand form: missing binding ids are generated,
 * missing `consumeOriginal` defaults to `true` (assigned keys are consumed),
 * and physical codes are normalized to canonical casing.
 */
export function parseProfileYaml(input: string): ProfileParseResult {
  let raw: unknown;
  try {
    raw = parseYaml(input);
  } catch {
    return { ok: false, error: "profile is not valid YAML" };
  }
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) {
    return { ok: false, error: "profile must be a mapping" };
  }

  const document = raw as Record<string, unknown>;
  const bindings = Array.isArray(document.bindings) ? document.bindings : [];
  const name = typeof document.name === "string" ? document.name : "";

  const normalized = {
    ...document,
    id: typeof document.id === "string" ? document.id : slugify(name),
    enabled: document.enabled ?? true,
    bindings: bindings.map((entry, index) => {
      const binding =
        typeof entry === "object" && entry !== null
          ? (entry as Record<string, unknown>)
          : {};
      return {
        ...binding,
        id:
          typeof binding.id === "string" && binding.id.length > 0
            ? binding.id
            : `binding-${index + 1}`,
        physicalCode: normalizePhysicalCode(String(binding.physicalCode ?? "")),
        consumeOriginal: binding.consumeOriginal ?? true,
      };
    }),
  };

  const result = profileSchema.safeParse(normalized);
  if (result.success) {
    return { ok: true, profile: result.data };
  }
  return { ok: false, error: formatZodError(result.error) };
}

/** Serializes a canonical profile to readable, shareable YAML. */
export function stringifyProfile(profile: Profile): string {
  return stringifyYaml(profile);
}

function slugify(input: string): string {
  return input
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

function formatZodError(error: ZodError): string {
  return error.issues
    .map((issue) => {
      const path = issue.path.join(".") || "(root)";
      return `${path}: ${issue.message}`;
    })
    .join("; ");
}
