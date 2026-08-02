import { describe, expect, it } from "vitest";
import { profileSchema } from "./index";

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

