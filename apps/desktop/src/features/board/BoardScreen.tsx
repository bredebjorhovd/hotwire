import { useCallback, useEffect, useRef, useState } from "react";

import type { Profile } from "@hotwire/schema";

import { Button } from "../../components/Button";
import { ActionRoute } from "../../components/ActionRoute";
import { ExecutionReceipt } from "../../components/ExecutionReceipt";
import { NumpadBoard } from "../../components/NumpadBoard";
import { SignalTrace } from "../../components/SignalTrace";
import { type KeycapState } from "../../components/VirtualKeycap";
import { actionShortLabel, getAction } from "../catalog/actions";
import { getAdapter } from "../catalog/adapters";
import { useReducedMotion } from "./useReducedMotion";
import {
  bindingForCode,
  physicalKeyLabel,
  routeForBinding,
  simulateExecution,
  type ExecutionReceiptData,
  type RouteStage,
} from "./execution";

type TraceState = "idle" | "running" | "done" | "failed";

export interface BoardScreenProps {
  profile: Profile;
}

/** The Board home screen (spec §8.1) shown after setup finishes. */
export function BoardScreen({ profile }: BoardScreenProps) {
  const reduced = useReducedMotion();
  const [selected, setSelected] = useState("Numpad5");
  const [receipt, setReceipt] = useState<ExecutionReceiptData | null>(null);
  const [stages, setStages] = useState<RouteStage[]>([]);
  const [traceState, setTraceState] = useState<TraceState>("idle");
  const [transient, setTransient] = useState<Record<string, KeycapState>>({});
  const [note, setNote] = useState<string | null>(null);
  const timers = useRef<number[]>([]);

  const clearTimers = () => {
    timers.current.forEach((id) => window.clearTimeout(id));
    timers.current = [];
  };

  useEffect(() => clearTimers, []);

  const trigger = useCallback(
    (code: string) => {
      const binding = bindingForCode(profile, code);
      if (!binding) return;
      clearTimers();
      const ms = (n: number) => (reduced ? 0 : n);
      setSelected(code);
      setTransient({ [code]: "pressed" });
      setStages(routeForBinding(profile, binding));
      setReceipt(simulateExecution(profile, binding));
      timers.current.push(
        window.setTimeout(() => {
          setTransient({ [code]: "triggered" });
          setTraceState("running");
          timers.current.push(
            window.setTimeout(() => {
              setTraceState("done");
              setTransient({});
              timers.current.push(
                window.setTimeout(() => setTraceState("idle"), ms(1400)),
              );
            }, ms(620)),
          );
        }, ms(70)),
      );
    },
    [profile, reduced],
  );

  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if (event.repeat) return;
      if (event.code.startsWith("Numpad") || event.code === "NumLock") {
        event.preventDefault();
        trigger(event.code);
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [trigger]);

  const binding = bindingForCode(profile, selected);
  const action = binding ? getAction(binding.actionId) : undefined;
  const adapter = binding ? getAdapter(binding.adapterId) : undefined;

  return (
    <div className="board-layout">
      <div>
        <div className="screen" style={{ paddingTop: 0 }}>
          <p className="eyebrow">Board</p>
          <h1 className="screen-title" style={{ marginTop: "var(--space-2)" }}>
            {profile.name}
            <span className="badge badge--ok" style={{ marginLeft: "var(--space-3)" }}>
              ● Active
            </span>
          </h1>
        </div>
        <NumpadBoard
          profile={profile}
          transient={transient}
          onPressKey={trigger}
          ariaLabel="Live board"
        />
        <div className="board-side" style={{ marginTop: "var(--space-4)" }}>
          {stages.length > 0 ? (
            <>
              <SignalTrace stages={stages} status={traceState} />
              <ActionRoute stages={stages} status={receipt?.status ?? "succeeded"} />
            </>
          ) : (
            <p className="prompt-note">
              Press a key — or the physical key on your keyboard — to fire it
              and trace the route.
            </p>
          )}
        </div>
      </div>

      <aside className="inspector" aria-label="Selected key">
        {binding && action && adapter ? (
          <>
            <div>
              <span className="key-id">{physicalKeyLabel(selected)}</span>
              <h2 className="key-name">
                {actionShortLabel(binding.actionId) ?? action.label}
              </h2>
              <p className="key-desc">{action.description}</p>
            </div>
            <div className="field">
              <span>Adapter</span>
              <b>{adapter.name}</b>
            </div>
            <div className="field">
              <span>Trigger</span>
              <b>{binding.trigger}</b>
            </div>
            <div className="callout-actions">
              <Button
                variant="primary"
                onClick={() => trigger(selected)}
              >
                Test
              </Button>
              <Button
                onClick={() =>
                  setNote("The action picker arrives with the Actions milestone.")
                }
              >
                Change action
              </Button>
            </div>
            {note && (
              <p className="prompt-note" role="status">
                {note}
              </p>
            )}
            {receipt && <ExecutionReceipt receipt={receipt} />}
          </>
        ) : (
          <div className="field">
            <span>Selected key</span>
            <b>No binding</b>
            <p className="key-desc">
              This key is unassigned. Select an assigned key on the board to
              inspect it.
            </p>
          </div>
        )}
      </aside>
    </div>
  );
}
