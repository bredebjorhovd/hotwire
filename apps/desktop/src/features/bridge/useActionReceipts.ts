import { useEffect, useState } from "react";

import { subscribeActionReceipts, type ActionReceipt } from "./ipc";

/**
 * Latest native `ActionReceipt` emitted by the Rust shell.
 *
 * Returns `null` until one arrives. The subscription is inert outside Tauri,
 * so the plain-vite prototype never sees a shell event.
 */
export function useActionReceipts(): ActionReceipt | null {
  const [receipt, setReceipt] = useState<ActionReceipt | null>(null);

  useEffect(() => {
    return subscribeActionReceipts(setReceipt);
  }, []);

  return receipt;
}
