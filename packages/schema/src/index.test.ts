import { describe, expect, it } from "vitest";
import {
  actionInvocationSchema,
  actionResultSchema,
  adapterManifestSchema,
  normalizePhysicalCode,
  profileSchema,
} from "./index";

describe("profileSchema", () => {
  it("accepts a readable v1 profile", () => {
    const result = profileSchema.safeParse({
      schemaVersion: 1,
      id: "ai-numpad",
      name: "AI Numpad",
      controlSurface: "numpad",
      bindings: [
        {
          id: "open-herdr",
          physicalCode: "Numpad5",
          trigger: "press",
          actionId: "app.open_or_focus",
          adapterId: "herdr",
          config: {},
          consumeOriginal: true,
        },
      ],
    });

    expect(result.success).toBe(true);
  });

  it("rejects unknown schema versions", () => {
    expect(
      profileSchema.safeParse({
        schemaVersion: 2,
        id: "future",
        name: "Future",
        controlSurface: "numpad",
        bindings: [],
      }).success,
    ).toBe(false);
  });
});

describe("normalizePhysicalCode", () => {
  it("canonicalizes numpad codes from arbitrary casing", () => {
    expect(normalizePhysicalCode("numpad5")).toBe("Numpad5");
    expect(normalizePhysicalCode(" NUMLOCK ")).toBe("NumLock");
  });

  it("passes unknown codes through trimmed", () => {
    expect(normalizePhysicalCode(" KeyA ")).toBe("KeyA");
  });
});

describe("adapter execution boundary", () => {
  it("validates an adapter manifest", () => {
    expect(
      adapterManifestSchema.safeParse({
        id: "herdr",
        name: "Herdr",
        version: "0.1.0",
        icon: "herdr",
        capabilities: ["focus", "new_task"],
        configSchema: {},
      }).success,
    ).toBe(true);
  });

  it("validates a full action invocation", () => {
    const result = actionInvocationSchema.safeParse({
      executionId: "exec-1",
      actionId: "app.open_or_focus",
      adapterId: "herdr",
      profileId: "ai-numpad",
      bindingId: "open-herdr",
      trigger: "press",
      config: { whenFocused: "new_task" },
      context: {
        profileId: "ai-numpad",
        bindingId: "open-herdr",
        trigger: "press",
        timestamp: "2026-08-02T00:00:00Z",
      },
    });

    expect(result.success).toBe(true);
  });

  it("validates an action result for the live board", () => {
    const result = actionResultSchema.safeParse({
      executionId: "exec-1",
      status: "succeeded",
      message: "Focused Herdr",
    });

    expect(result.success).toBe(true);
  });
});
