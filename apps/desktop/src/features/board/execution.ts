/**
 * Board execution model for the Milestone 0 prototype.
 *
 * Pure, platform-neutral functions that resolve a physical code to a binding,
 * build the four-stage route (physical key → action → adapter → result) and
 * produce an execution receipt. The prototype simulates execution with fixture
 * data only; a native `hotwire-core` boundary will replace this later.
 */

import type { Binding, Profile } from "@hotwire/schema";

import { getAction, actionShortLabel } from "../catalog/actions";
import { getAdapter } from "../catalog/adapters";

export type ExecutionStatus = "succeeded" | "failed";

export type FocusDirection = "up" | "down" | "left" | "right";

/** One stage of the signal-trace route. */
export interface RouteStage {
  label: string;
  detail: string;
}

/** The complete receipt shown after a binding fires (spec §12.3 / §4.1 S7). */
export interface ExecutionReceiptData {
  executionId: string;
  physicalKey: string;
  action: string;
  adapter: string;
  result: string;
  status: ExecutionStatus;
  message: string;
  trigger: string;
  timestamp: string;
}

/** Display label for a numpad physical code (route receipts, keycaps). */
export const physicalKeyLabels: Record<string, string> = {
  Numpad0: "NUM 0",
  Numpad1: "NUM 1",
  Numpad2: "NUM 2",
  Numpad3: "NUM 3",
  Numpad4: "NUM 4",
  Numpad5: "NUM 5",
  Numpad6: "NUM 6",
  Numpad7: "NUM 7",
  Numpad8: "NUM 8",
  Numpad9: "NUM 9",
  NumpadAdd: "ADD",
  NumpadSubtract: "SUBTRACT",
  NumpadMultiply: "MULTIPLY",
  NumpadDivide: "DIVIDE",
  NumpadDecimal: "DECIMAL",
  NumpadEnter: "ENTER",
  NumLock: "NUM LOCK",
};

/** Short engraved legend printed on a keycap. */
export const physicalKeySublabels: Record<string, string> = {
  Numpad0: "0",
  Numpad1: "1",
  Numpad2: "2",
  Numpad3: "3",
  Numpad4: "4",
  Numpad5: "5",
  Numpad6: "6",
  Numpad7: "7",
  Numpad8: "8",
  Numpad9: "9",
  NumpadAdd: "+",
  NumpadSubtract: "-",
  NumpadMultiply: "×",
  NumpadDivide: "÷",
  NumpadDecimal: ".",
  NumpadEnter: "⏎",
  NumLock: "⇭",
};

export function physicalKeyLabel(code: string): string {
  return physicalKeyLabels[code] ?? code.toUpperCase();
}

export function physicalKeySublabel(code: string): string {
  return physicalKeySublabels[code] ?? "";
}

/** Resolves the first enabled binding assigned to a physical code. */
export function bindingForCode(
  profile: Profile,
  code: string,
): Binding | undefined {
  return profile.bindings.find(
    (binding) => binding.physicalCode === code && binding.enabled,
  );
}

/** Result stage label, mirroring spec examples (`FOCUSED`, `SHORTCUT SENT`). */
export function resultLabelFor(actionId: string): string {
  switch (actionId) {
    case "voice.input":
      return "SHORTCUT SENT";
    case "app.open_or_focus":
      return "FOCUSED";
    case "agent.new":
      return "AGENT STARTED";
    case "agent.continue":
      return "CONTINUED";
    case "agent.plan":
      return "PLAN REQUESTED";
    case "agent.review":
      return "REVIEW REQUESTED";
    case "agent.accept":
      return "ACCEPTED";
    case "agent.reject":
      return "REJECTED";
    case "test.run":
      return "TESTS STARTED";
    case "terminal.open":
      return "TERMINAL OPENED";
    case "git.diff":
      return "DIFF SHOWN";
    case "git.commit":
      return "COMMITTED";
    case "git.pr":
      return "PR OPENED";
    case "claude.launch":
    case "codex.launch":
      return "LAUNCHED";
    case "profile.switch":
      return "PROFILE SWITCHED";
    default:
      return "EXECUTED";
  }
}

function resultMessageFor(binding: Binding): string {
  const action = getAction(binding.actionId);
  const adapter = getAdapter(binding.adapterId);
  const actionName = action?.label ?? binding.actionId;
  const adapterName = adapter?.name ?? binding.adapterId;
  switch (binding.actionId) {
    case "voice.input":
      return "Held the Papegøye push-to-talk shortcut";
    case "app.open_or_focus":
      return `Focused ${adapterName}`;
    case "git.commit":
      return "Staged changes committed";
    case "test.run":
      return "Tests started in the terminal";
    default:
      return `${actionName} sent through ${adapterName}`;
  }
}

/** Builds the four-stage route for a binding's signal trace. */
export function routeForBinding(
  profile: Profile,
  binding: Binding,
): RouteStage[] {
  const action = getAction(binding.actionId);
  const adapter = getAdapter(binding.adapterId);
  const actionName =
    actionShortLabel(binding.actionId) ?? action?.label ?? binding.actionId;
  return [
    { label: physicalKeyLabel(binding.physicalCode), detail: "Physical key" },
    { label: actionName, detail: "Action" },
    { label: (adapter?.name ?? binding.adapterId).toUpperCase(), detail: "Adapter" },
    { label: resultLabelFor(binding.actionId), detail: "Result" },
  ];
}

let executionCounter = 0;

/** Simulates executing a binding and returns a receipt for the live board. */
export function simulateExecution(
  profile: Profile,
  binding: Binding,
): ExecutionReceiptData {
  executionCounter += 1;
  const action = getAction(binding.actionId);
  const adapter = getAdapter(binding.adapterId);
  return {
    executionId: `exec-${executionCounter.toString().padStart(3, "0")}`,
    physicalKey: physicalKeyLabel(binding.physicalCode),
    action: actionShortLabel(binding.actionId) ?? action?.label ?? binding.actionId,
    adapter: (adapter?.name ?? binding.adapterId).toUpperCase(),
    result: resultLabelFor(binding.actionId),
    status: "succeeded",
    message: resultMessageFor(binding),
    trigger: binding.trigger,
    timestamp: new Date().toISOString(),
  };
}

export const NUMPAD_LAYOUT: Array<{
  code: string;
  position?: "row3";
  span?: 2;
}> = [
  { code: "NumLock" },
  { code: "NumpadDivide" },
  { code: "NumpadMultiply" },
  { code: "NumpadSubtract" },
  { code: "Numpad7" },
  { code: "Numpad8" },
  { code: "Numpad9" },
  { code: "NumpadAdd" },
  { code: "Numpad4" },
  { code: "Numpad5" },
  { code: "Numpad6" },
  { code: "NumpadEnter", position: "row3" },
  { code: "Numpad1" },
  { code: "Numpad2" },
  { code: "Numpad3" },
  { code: "Numpad0", span: 2 },
  { code: "NumpadDecimal" },
];

/**
 * Grid coordinates used for arrow-key navigation. The Enter key spans two
 * rows (3–4) and the wide zero spans two columns (1–2); the first cell listed
 * is the anchor used for neighbor math.
 */
const numpadCells: Record<string, Array<[number, number]>> = {
  NumLock: [[1, 1]],
  NumpadDivide: [[1, 2]],
  NumpadMultiply: [[1, 3]],
  NumpadSubtract: [[1, 4]],
  Numpad7: [[2, 1]],
  Numpad8: [[2, 2]],
  Numpad9: [[2, 3]],
  NumpadAdd: [[2, 4]],
  Numpad4: [[3, 1]],
  Numpad5: [[3, 2]],
  Numpad6: [[3, 3]],
  NumpadEnter: [[3, 4], [4, 4]],
  Numpad1: [[4, 1]],
  Numpad2: [[4, 2]],
  Numpad3: [[4, 3]],
  Numpad0: [[5, 1], [5, 2]],
  NumpadDecimal: [[5, 3]],
};

const cellToCode = new Map<string, string>();
for (const [code, cells] of Object.entries(numpadCells)) {
  for (const [row, col] of cells) {
    cellToCode.set(`${row},${col}`, code);
  }
}

/**
 * Returns the adjacent key for a focus movement, or `null` when there is none
 * (used by the live board's arrow-key navigation). Keys that span multiple
 * cells (the wide zero, the tall enter) step past their own covered cells so
 * the movement still lands on the neighbouring key.
 */
export function numpadNeighbor(
  code: string,
  direction: FocusDirection,
): string | null {
  const anchor = numpadCells[code]?.[0];
  if (!anchor) return null;
  const [row, col] = anchor;
  const deltaRow = direction === "up" ? -1 : direction === "down" ? 1 : 0;
  const deltaCol = direction === "left" ? -1 : direction === "right" ? 1 : 0;
  for (let step = 1; step <= 6; step += 1) {
    const target = cellToCode.get(`${row + deltaRow * step},${col + deltaCol * step}`);
    if (target === undefined) return null;
    if (target !== code) return target;
  }
  return null;
}
