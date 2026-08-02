import type { ExecutionReceiptData } from "../features/board/execution";

export interface ExecutionReceiptProps {
  receipt: ExecutionReceiptData;
}

/** Post-execution receipt panel for the live board (spec §6.4 diagnostics). */
export function ExecutionReceipt({ receipt }: ExecutionReceiptProps) {
  const failed = receipt.status === "failed";
  return (
    <div className="receipt" role="region" aria-label="Execution receipt">
      <div className="receipt-header">
        <b>Execution receipt</b>
        <span
          className={`receipt-status-dot ${failed ? "failed" : ""}`}
          aria-hidden="true"
        />
      </div>
      <div className="receipt-grid">
        <div className="receipt-field">
          <span>Key</span>
          <b>{receipt.physicalKey}</b>
        </div>
        <div className="receipt-field">
          <span>Action</span>
          <b>{receipt.action}</b>
        </div>
        <div className="receipt-field">
          <span>Adapter</span>
          <b>{receipt.adapter}</b>
        </div>
        <div className="receipt-field">
          <span>Result</span>
          <b className={failed ? "failed" : "success"}>{receipt.result}</b>
        </div>
        <div className="receipt-field">
          <span>Trigger</span>
          <b>{receipt.trigger.toUpperCase()}</b>
        </div>
      </div>
      <div className="receipt-field">
        <span>Message</span>
        <b>{receipt.message}</b>
      </div>
    </div>
  );
}
