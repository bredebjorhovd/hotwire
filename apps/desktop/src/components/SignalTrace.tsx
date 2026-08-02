import type { CSSProperties } from "react";

import type { RouteStage } from "../features/board/execution";

export type TraceStatus = "idle" | "running" | "done" | "failed";

export interface SignalTraceProps {
  stages: RouteStage[];
  /** What the trace is doing: idle = dim, running = signal travels, done/failed = settled. */
  status?: TraceStatus;
  ariaLabel?: string;
}

/**
 * The animated signal trace (spec §7.7): a fine amber line runs
 * physical key → semantic action → adapter → result over 420–600ms. Under
 * reduced motion the CSS tokens collapse the delays, so nodes light in place.
 */
export function SignalTrace({
  stages,
  status = "idle",
  ariaLabel = "Action route",
}: SignalTraceProps) {
  return (
    <ol className="trace" aria-label={ariaLabel}>
      {stages.map((stage, index) => {
        const last = index === stages.length - 1;
        let nodeState = "pending";
        if (status === "done") nodeState = "done";
        else if (status === "failed") nodeState = last ? "failed" : "done";
        else if (status === "running") nodeState = "active";
        return (
          <li
            className="trace-step"
            key={`${stage.detail}-${index}`}
            style={{ "--i": index } as CSSProperties}
          >
            <span className="trace-node" data-state={nodeState}>
              <span className="trace-dot" aria-hidden="true" />
              <span className="trace-main">{stage.label}</span>
              <span className="trace-sub">{stage.detail}</span>
            </span>
            {!last && (
              <span
                className="trace-line"
                data-filled={status === "done" || status === "failed" ? "true" : "false"}
                aria-hidden="true"
              />
            )}
          </li>
        );
      })}
    </ol>
  );
}
