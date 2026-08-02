import { z } from "zod";

export const triggerSchema = z.enum(["press", "hold", "double_press"]);

export const bindingSchema = z.object({
  id: z.string().min(1),
  physicalCode: z.string().min(1),
  trigger: triggerSchema,
  actionId: z.string().min(1),
  adapterId: z.string().min(1),
  config: z.record(z.string(), z.unknown()).default({}),
  consumeOriginal: z.boolean(),
  enabled: z.boolean().default(true),
});

export const profileSchema = z.object({
  schemaVersion: z.literal(1),
  id: z.string().min(1),
  name: z.string().min(1),
  controlSurface: z.enum(["numpad", "function_row", "manual"]),
  bindings: z.array(bindingSchema),
  layerKey: z.string().min(1).optional(),
  enabled: z.boolean().default(true),
});

export type Binding = z.infer<typeof bindingSchema>;
export type Profile = z.infer<typeof profileSchema>;

