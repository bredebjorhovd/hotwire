import { describe, expect, it } from "vitest";

import type { ActionReceipt } from "./ipc";
import { receiptRouteLabel } from "./receipts";

const receipt: ActionReceipt = {
  executionId: "mock-001",
  profileId: "ai-numpad",
  bindingId: "b-numpad5-herdr",
  physicalCode: "Numpad5",
  actionId: "app.open_or_focus",
  adapterId: "herdr",
  status: "succeeded",
  message: "Focused Herdr",
};

describe("receiptRouteLabel", () => {
  it("renders the first-slice route readout", () => {
    expect(receiptRouteLabel(receipt)).toBe(
      "NUM 5 → OPEN OR FOCUS → HERDR → SUCCEEDED",
    );
  });

  it("keeps unknown adapters and actions readable", () => {
    const unknown = {
      ...receipt,
      actionId: "custom.do_thing",
      adapterId: "custom",
      status: "failed" as const,
    };
    expect(receiptRouteLabel(unknown)).toBe(
      "NUM 5 → CUSTOM.DO_THING → CUSTOM → FAILED",
    );
  });
});
