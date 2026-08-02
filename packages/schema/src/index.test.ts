import { describe, expect, it } from "vitest";
import {
  actionInvocationSchema,
  actionResultSchema,
  adapterManifestSchema,
  bindingSchema,
  herdrConfigSchema,
  normalizePhysicalCode,
  papegoyeConfigSchema,
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

  it("defaults captureMode to capture and layer to false", () => {
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
    if (!result.success) return;
    expect(result.data.captureMode).toBe("capture");
    expect(result.data.bindings[0]?.layer).toBe(false);
  });

  it("accepts a layer binding and every capture mode", () => {
    for (const captureMode of ["capture", "modified_capture", "passthrough"]) {
      const result = profileSchema.safeParse({
        schemaVersion: 1,
        id: "layered",
        name: "Layered",
        controlSurface: "numpad",
        layerKey: "NumLock",
        captureMode,
        bindings: [
          {
            id: "alternate",
            physicalCode: "Numpad7",
            trigger: "press",
            actionId: "app.alternate",
            adapterId: "herdr",
            config: {},
            consumeOriginal: true,
            layer: true,
          },
        ],
      });

      expect(result.success, `captureMode=${captureMode}`).toBe(true);
      if (result.success) {
        expect(result.data.bindings[0]?.layer).toBe(true);
      }
    }
  });

  it("rejects an invalid capture mode", () => {
    expect(
      profileSchema.safeParse({
        schemaVersion: 1,
        id: "bad",
        name: "Bad",
        controlSurface: "numpad",
        captureMode: "everything",
        bindings: [],
      }).success,
    ).toBe(false);
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

describe("bindingSchema", () => {
  it("is inert when layer is omitted", () => {
    const result = bindingSchema.safeParse({
      id: "b",
      physicalCode: "Numpad5",
      trigger: "press",
      actionId: "app.x",
      adapterId: "herdr",
      consumeOriginal: true,
    });

    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.layer).toBe(false);
    }
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

describe("herdrConfigSchema", () => {
  it("accepts each integration path", () => {
    expect(herdrConfigSchema.safeParse({ apiBaseUrl: "http://127.0.0.1:7398" }).success).toBe(true);
    expect(herdrConfigSchema.safeParse({ deepLink: "herdr://actions/focus" }).success).toBe(true);
    expect(herdrConfigSchema.safeParse({ bundleId: "dev.herdr.app" }).success).toBe(true);
    expect(herdrConfigSchema.safeParse({ appPath: "/Applications/Herdr.app" }).success).toBe(true);
    expect(herdrConfigSchema.safeParse({ shortcut: "F17" }).success).toBe(true);
  });

  it("requires at least one integration path", () => {
    const result = herdrConfigSchema.safeParse({});
    expect(result.success).toBe(false);
    if (!result.success) {
      expect(JSON.stringify(result.error.issues)).toContain("at least one integration path");
    }
  });

  it("rejects non-loopback API URLs and scheme-less deep links", () => {
    expect(herdrConfigSchema.safeParse({ apiBaseUrl: "https://example.com" }).success).toBe(false);
    expect(herdrConfigSchema.safeParse({ deepLink: "focus" }).success).toBe(false);
  });
});

describe("papegoyeConfigSchema", () => {
  it("accepts a shortcut or a keycode", () => {
    expect(papegoyeConfigSchema.safeParse({ shortcut: "fn+space" }).success).toBe(true);
    expect(papegoyeConfigSchema.safeParse({ keycode: 64 }).success).toBe(true);
    expect(
      papegoyeConfigSchema.safeParse({ keycode: 64, modifiers: ["fn"] }).success,
    ).toBe(true);
  });

  it("requires exactly one of shortcut or keycode", () => {
    expect(papegoyeConfigSchema.safeParse({}).success).toBe(false);
    expect(
      papegoyeConfigSchema.safeParse({ shortcut: "fn+space", keycode: 64 }).success,
    ).toBe(false);
  });

  it("rejects unknown modifiers", () => {
    const result = papegoyeConfigSchema.safeParse({
      keycode: 64,
      modifiers: ["mega"],
    });
    expect(result.success).toBe(false);
  });
});
