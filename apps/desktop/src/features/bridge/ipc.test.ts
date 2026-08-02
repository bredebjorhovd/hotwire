import { describe, expect, it } from "vitest";

import {
  ACTION_RECEIPT_EVENT,
  cancelAdapterAction,
  detectAdapter,
  emitMockActionReceipt,
  getAppStatus,
  getDiagnostics,
  isRunningInTauri,
  pauseCapture,
  quitDesktop,
  releaseAdapterAction,
  resumeCapture,
  runAdapterAction,
  showMainWindow,
  subscribeActionReceipts,
  validateAdapterConfig,
  validateProfileYaml,
  type ActionReceipt,
} from "./ipc";

const validYaml = `schemaVersion: 1
id: ai-numpad
name: AI Numpad
controlSurface: numpad
bindings:
  - physicalCode: Numpad5
    trigger: press
    actionId: app.open_or_focus
    adapterId: herdr
    consumeOriginal: true
`;

const invalidYaml = "bindings: [not a profile]";

describe("ipc bridge outside Tauri", () => {
  it("detects that the page is not running inside Tauri", () => {
    expect(isRunningInTauri()).toBe(false);
  });

  it("reports a browser-preview status", async () => {
    await expect(getAppStatus()).resolves.toMatchObject({
      appVersion: "browser-preview",
      inputBackend: "none",
      captureAvailable: false,
    });
  });

  it("validates profiles with the shared fallback validator", async () => {
    await expect(validateProfileYaml(validYaml)).resolves.toMatchObject({
      valid: true,
    });
    await expect(validateProfileYaml(invalidYaml)).resolves.toMatchObject({
      valid: false,
    });
  });

  it("degrades lifecycle commands to no-ops", async () => {
    await expect(showMainWindow()).resolves.toBeUndefined();
    await expect(quitDesktop()).resolves.toBeUndefined();
  });

  it("never fakes a mock receipt without the shell", async () => {
    await expect(emitMockActionReceipt()).resolves.toBeNull();
  });

  it("reports a safe diagnostics preview with no sensitive detail", async () => {
    await expect(getDiagnostics()).resolves.toMatchObject({
      appVersion: "browser-preview",
      capture: {
        permission: "authorized",
        status: "stopped",
        paused: false,
      },
    });
  });

  it("degrades pause and resume to browser no-ops", async () => {
    await expect(pauseCapture()).resolves.toBe(false);
    await expect(resumeCapture()).resolves.toBe(false);
  });

  it("degrades adapter commands to no-ops without the shell", async () => {
    await expect(
      runAdapterAction("herdr", "app.open_or_focus", "press", {}, "Numpad5"),
    ).resolves.toBeNull();
    await expect(
      releaseAdapterAction("papegoye", "exec-1", "Numpad0"),
    ).resolves.toBeNull();
    await expect(
      cancelAdapterAction("papegoye", "exec-1", "Numpad0"),
    ).resolves.toBeNull();
    await expect(detectAdapter("herdr")).resolves.toBeNull();
    await expect(validateAdapterConfig("herdr", {})).resolves.toBeNull();
  });

  it("subscribing outside Tauri returns a safe no-op unsubscriber", () => {
    const received: ActionReceipt[] = [];
    const unsubscribe = subscribeActionReceipts((receipt) => received.push(receipt));
    expect(received).toEqual([]);
    expect(() => unsubscribe()).not.toThrow();
  });

  it("exposes the stable event name for both sides", () => {
    expect(ACTION_RECEIPT_EVENT).toBe("action-receipt");
  });
});
