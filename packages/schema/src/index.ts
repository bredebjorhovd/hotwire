import { z } from "zod";

/**
 * Shared, versioned TypeScript boundary types for Hotwire.
 *
 * Mirrors the Rust model in `crates/` (see `docs/architecture.md`). The
 * physical-key event model is owned by `hotwire-core`; this package owns the
 * profile document, the semantic-action catalog, and the adapter-execution
 * contract as the frontend sees them.
 */

/** The physical interaction a binding is triggered by (spec §9.1). */
export const triggerSchema = z.enum(["press", "hold", "double_press"]);
export type Trigger = z.infer<typeof triggerSchema>;

/** How a profile treats key events that match its bindings (spec §9.3). */
export const captureModeSchema = z.enum([
  "capture",
  "modified_capture",
  "passthrough",
]);
export type CaptureMode = z.infer<typeof captureModeSchema>;

/** Canonical physical codes for a standard numpad (scan-code based). */
export const numpadPhysicalCodes = [
  "Numpad0",
  "Numpad1",
  "Numpad2",
  "Numpad3",
  "Numpad4",
  "Numpad5",
  "Numpad6",
  "Numpad7",
  "Numpad8",
  "Numpad9",
  "NumpadAdd",
  "NumpadSubtract",
  "NumpadMultiply",
  "NumpadDivide",
  "NumpadDecimal",
  "NumpadEnter",
  "NumLock",
] as const;

export type NumpadPhysicalCode = (typeof numpadPhysicalCodes)[number];

export const numpadPhysicalCodeSchema = z.enum(numpadPhysicalCodes);

/**
 * Normalizes a user- or OS-supplied physical-code string to its canonical
 * casing. Unknown codes pass through trimmed so custom layouts still work.
 */
export function normalizePhysicalCode(input: string): string {
  const trimmed = input.trim();
  const match = numpadPhysicalCodes.find(
    (code) => code.toLowerCase() === trimmed.toLowerCase(),
  );
  return match ?? trimmed;
}

export const bindingSchema = z.object({
  id: z.string().min(1),
  physicalCode: z.string().min(1),
  trigger: triggerSchema,
  actionId: z.string().min(1),
  adapterId: z.string().min(1),
  config: z.record(z.string(), z.unknown()).default({}),
  consumeOriginal: z.boolean(),
  enabled: z.boolean().default(true),
  /**
   * Only fires while the profile's layer key is held (spec §9.2 alternate
   * action). Inert when the profile has no `layerKey`.
   */
  layer: z.boolean().default(false),
});

export const profileSchema = z.object({
  schemaVersion: z.literal(1),
  id: z.string().min(1),
  name: z.string().min(1),
  controlSurface: z.enum(["numpad", "function_row", "manual"]),
  bindings: z.array(bindingSchema),
  layerKey: z.string().min(1).optional(),
  captureMode: captureModeSchema.default("capture"),
  enabled: z.boolean().default(true),
});

export type Binding = z.infer<typeof bindingSchema>;
export type Profile = z.infer<typeof profileSchema>;

/** Semantic actions a key can be assigned to. */
export const actionDefinitionSchema = z.object({
  id: z.string().min(1),
  label: z.string().min(1),
  description: z.string().min(1),
  icon: z.string().min(1),
  category: z.string().min(1),
  risk: z.enum(["none", "low", "confirmation"]),
  supportedTriggers: z.array(triggerSchema),
});

export type ActionDefinition = z.infer<typeof actionDefinitionSchema>;

/** Static identity and capabilities of an adapter. */
export const adapterManifestSchema = z.object({
  id: z.string().min(1),
  name: z.string().min(1),
  version: z.string().min(1),
  icon: z.string().min(1),
  capabilities: z.array(z.string()),
  configSchema: z.unknown(),
});

export type AdapterManifest = z.infer<typeof adapterManifestSchema>;

/** The foreground application, when the OS can identify one. */
export const activeApplicationSchema = z.object({
  bundleId: z.string().optional(),
  processName: z.string(),
});

/** Everything an adapter needs to know about the moment an action fired. */
export const executionContextSchema = z.object({
  activeApplication: activeApplicationSchema.optional(),
  cwd: z.string().optional(),
  profileId: z.string(),
  bindingId: z.string(),
  trigger: triggerSchema,
  timestamp: z.string(),
});

export type ExecutionContext = z.infer<typeof executionContextSchema>;

/** A concrete request to run one semantic action through an adapter. */
export const actionInvocationSchema = z.object({
  executionId: z.string(),
  actionId: z.string(),
  adapterId: z.string(),
  profileId: z.string(),
  bindingId: z.string(),
  trigger: triggerSchema,
  config: z.record(z.string(), z.unknown()),
  context: executionContextSchema,
});

export type ActionInvocation = z.infer<typeof actionInvocationSchema>;

export const actionResultStatusSchema = z.enum([
  "started",
  "succeeded",
  "failed",
  "cancelled",
]);

/** The outcome of an adapter execution, ready for the live board. */
export const actionResultSchema = z.object({
  executionId: z.string(),
  status: actionResultStatusSchema,
  message: z.string().optional(),
});

export type ActionResult = z.infer<typeof actionResultSchema>;
