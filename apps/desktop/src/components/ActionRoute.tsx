import type { RouteStage } from "../features/board/execution";

export interface ActionRouteProps {
  stages: RouteStage[];
  status?: "succeeded" | "failed";
  ariaLabel?: string;
}

/**
 * Static route receipt (spec §4.1 S7): a compact monospaced readout such as
 * `NUM 5 → OPEN OR FOCUS → HERDR → FOCUSED`. Used on the test screen and the
 * board footer; the animated equivalent is `SignalTrace`.
 */
export function ActionRoute({
  stages,
  status = "succeeded",
  ariaLabel = "Action route",
}: ActionRouteProps) {
  return (
    <div
      className="route-receipt"
      role="status"
      aria-label={ariaLabel}
    >
      {stages.map((stage, index) => {
        const last = index === stages.length - 1;
        return (
          <span
            key={`${stage.detail}-${index}`}
            data-role={last ? "result" : stage.detail.toLowerCase()}
          >
            <span
              className={last ? `route-result ${status === "failed" ? "failed" : ""}` : undefined}
            >
              {stage.label}
            </span>
            {!last && <span className="route-arrow" aria-hidden="true">→</span>}
          </span>
        );
      })}
    </div>
  );
}
