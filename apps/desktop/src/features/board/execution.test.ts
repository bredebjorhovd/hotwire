import { describe, expect, it } from "vitest";

import { loadFixtureProfile } from "../catalog/fixtures";
import {
  bindingForCode,
  numpadNeighbor,
  physicalKeyLabel,
  routeForBinding,
  simulateExecution,
} from "./execution";

describe("execution model (happy path)", () => {
  const profile = loadFixtureProfile("ai-numpad");

  it("resolves an enabled binding from a physical code", () => {
    const binding = bindingForCode(profile, "Numpad5");
    expect(binding?.adapterId).toBe("herdr");
    expect(binding?.actionId).toBe("app.open_or_focus");
    expect(bindingForCode(profile, "KeyA")).toBeUndefined();
  });

  it("builds a full physical key → action → adapter → result route", () => {
    const binding = bindingForCode(profile, "Numpad0");
    expect(binding).toBeDefined();
    const stages = routeForBinding(profile, binding!);

    expect(stages.map((stage) => stage.detail)).toEqual([
      "Physical key",
      "Action",
      "Adapter",
      "Result",
    ]);
    expect(stages.map((stage) => stage.label)).toEqual([
      "NUM 0",
      "VOICE",
      "PAPEGØYE",
      "SHORTCUT SENT",
    ]);
  });

  it("simulates an execution and returns a receipt", () => {
    const binding = bindingForCode(profile, "Numpad5");
    expect(binding).toBeDefined();
    const receipt = simulateExecution(profile, binding!);

    expect(receipt.status).toBe("succeeded");
    expect(receipt.physicalKey).toBe("NUM 5");
    expect(receipt.action).toBe("OPEN OR FOCUS");
    expect(receipt.adapter).toBe("HERDR");
    expect(receipt.result).toBe("FOCUSED");
    expect(receipt.trigger).toBe("press");
    expect(receipt.message).toContain("Focused");
  });

  it("maps physical codes to display labels", () => {
    expect(physicalKeyLabel("NumpadEnter")).toBe("ENTER");
    expect(physicalKeyLabel("Numpad7")).toBe("NUM 7");
    expect(physicalKeyLabel("NumLock")).toBe("NUM LOCK");
  });

  it("navigates the numpad grid by arrow direction", () => {
    expect(numpadNeighbor("Numpad5", "up")).toBe("Numpad8");
    expect(numpadNeighbor("Numpad5", "down")).toBe("Numpad2");
    expect(numpadNeighbor("Numpad5", "left")).toBe("Numpad4");
    expect(numpadNeighbor("Numpad6", "right")).toBe("NumpadEnter");
    expect(numpadNeighbor("Numpad0", "right")).toBe("NumpadDecimal");
    expect(numpadNeighbor("NumLock", "up")).toBeNull();
  });
});
