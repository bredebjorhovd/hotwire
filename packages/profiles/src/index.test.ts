import { describe, expect, it } from "vitest";

import { listFixtures, loadFixtureProfile } from "./fixtures";
import { parseProfileYaml, stringifyProfile } from "./index";

describe("profile fixtures", () => {
  it("every fixture validates against the canonical schema", () => {
    for (const name of listFixtures()) {
      const result = loadFixtureProfile(name);
      expect(result.ok, `${name}: ${result.ok ? "" : result.error}`).toBe(true);
    }
  });

  it("ai-numpad carries the signature bindings", () => {
    const result = loadFixtureProfile("ai-numpad.yaml");
    expect(result.ok).toBe(true);
    if (!result.ok) return;

    expect(result.profile.layerKey).toBe("NumLock");
    expect(result.profile.bindings).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ physicalCode: "Numpad5", adapterId: "herdr" }),
        expect.objectContaining({ physicalCode: "Numpad0", trigger: "hold" }),
      ]),
    );
  });

  it("canonical profiles round-trip through YAML export", () => {
    for (const name of listFixtures()) {
      const loaded = loadFixtureProfile(name);
      expect(loaded.ok).toBe(true);
      if (!loaded.ok) continue;

      const exported = stringifyProfile(loaded.profile);
      const reparsed = parseProfileYaml(exported);
      expect(reparsed.ok, `${name}: ${reparsed.ok ? "" : reparsed.error}`).toBe(
        true,
      );
      if (reparsed.ok) {
        expect(reparsed.profile).toEqual(loaded.profile);
      }
    }
  });
});

describe("parseProfileYaml", () => {
  it("normalizes spec-style shorthand into a canonical profile", () => {
    const result = parseProfileYaml(`
schemaVersion: 1
name: Quick
controlSurface: numpad
bindings:
  - physicalCode: numpad5
    trigger: press
    actionId: app.open_or_focus
    adapterId: herdr
    consumeOriginal: false
`);

    expect(result.ok).toBe(true);
    if (!result.ok) return;

    expect(result.profile.id).toBe("quick");
    expect(result.profile.bindings[0]?.physicalCode).toBe("Numpad5");
    expect(result.profile.bindings[0]?.id).toBe("binding-1");
    expect(result.profile.bindings[0]?.consumeOriginal).toBe(false);
  });

  it("defaults consumeOriginal to true when omitted", () => {
    const result = parseProfileYaml(`
schemaVersion: 1
id: quick
name: Quick
controlSurface: numpad
bindings:
  - physicalCode: Numpad5
    trigger: press
    actionId: app.open_or_focus
    adapterId: herdr
`);

    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.profile.bindings[0]?.consumeOriginal).toBe(true);
  });

  it("rejects unknown schema versions with a readable error", () => {
    const result = parseProfileYaml("schemaVersion: 2\nname: Future\n");

    expect(result.ok).toBe(false);
    if (result.ok) return;
    expect(result.error).toContain("schemaVersion");
  });

  it("defaults captureMode and layer when omitted", () => {
    const result = parseProfileYaml(`
schemaVersion: 1
id: quick
name: Quick
controlSurface: numpad
bindings:
  - physicalCode: Numpad5
    trigger: press
    actionId: app.open_or_focus
    adapterId: herdr
`);

    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.profile.captureMode).toBe("capture");
    expect(result.profile.bindings[0]?.layer).toBe(false);
  });

  it("accepts a layer key, capture mode, and layer binding", () => {
    const result = parseProfileYaml(`
schemaVersion: 1
id: layered
name: Layered
controlSurface: numpad
layerKey: NumLock
captureMode: modified_capture
bindings:
  - physicalCode: Numpad7
    trigger: press
    actionId: app.alternate
    adapterId: herdr
    consumeOriginal: true
    layer: true
`);

    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.profile.layerKey).toBe("NumLock");
    expect(result.profile.captureMode).toBe("modified_capture");
    expect(result.profile.bindings[0]?.layer).toBe(true);
  });

  it("rejects non-mapping documents", () => {
    expect(parseProfileYaml("- just\n- a\n- list").ok).toBe(false);
  });
});
