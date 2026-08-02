/**
 * Display mapping for native `ActionReceipt` events.
 *
 * Turns the `hotwire-core` payload the shell emits into the same route-style
 * readout the board uses (`NUM 5 → OPEN OR FOCUS → HERDR → SUCCEEDED`), so the
 * shell-driven receipt and the fixture-driven prototype render consistently.
 */

import { actionShortLabel } from "../catalog/actions";
import { getAdapter } from "../catalog/adapters";
import { physicalKeyLabel } from "../board/execution";
import type { ActionReceipt } from "./ipc";

/** Compact route readout for a native action receipt. */
export function receiptRouteLabel(receipt: ActionReceipt): string {
  const action = actionShortLabel(receipt.actionId) ?? receipt.actionId;
  const adapter = (getAdapter(receipt.adapterId)?.name ?? receipt.adapterId).toUpperCase();
  return [
    physicalKeyLabel(receipt.physicalCode),
    action.toUpperCase(),
    adapter,
    receipt.status.toUpperCase(),
  ].join(" → ");
}

/** Short status word, e.g. `SUCCEEDED`; unknown statuses pass through. */
export function receiptStatusLabel(status: ActionReceipt["status"]): string {
  return status.toUpperCase();
}
