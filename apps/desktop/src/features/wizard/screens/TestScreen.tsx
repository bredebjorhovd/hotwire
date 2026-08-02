import { useCallback, useEffect, useRef, useState } from "react";

import type { Profile } from "@hotwire/schema";

import { ActionRoute } from "../../../components/ActionRoute";
import { ExecutionReceipt } from "../../../components/ExecutionReceipt";
import { NumpadBoard } from "../../../components/NumpadBoard";
import { SignalTrace } from "../../../components/SignalTrace";
import { type KeycapState } from "../../../components/VirtualKeycap";
import {
  bindingForCode,
  routeForBinding,
  simulateExecution,
  type ExecutionReceiptData,
  type RouteStage,
} from "../../board/execution";
import { useReducedMotion } from "../../board/useReducedMotion";

export type TraceState = "idle" | "running" | "done" | "failed";

export interface TestScreenProps {
  profile: Profile;
  onTested?: () => void;
}

/**
 * Screen 7 — Test the board (spec §4.1). The live numpad sits centre stage;
 * pressing a physical key (or clicking one) animates it and shows the complete
 * physical key → action → adapter → result route. Matched events are consumed
 * so numeric input never reaches the app during the test.
 */
export function TestScreen({ profile, onTested }: TestScreenProps) {
  const reduced = useReducedMotion();
  const [receipt, setReceipt] = useState<ExecutionReceiptData | null>(null);
  const [stages, setStages] = useState<RouteStage[]>([]);
  const [traceState, setTraceState] = useState<TraceState>("idle");
  const [transient, setTransient] = useState<Record<string, KeycapState>>({});
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
                window.setTimeout(
                  () => setTraceState("idle"),
                  ms(1400),
                ),
              );
            }, ms(620)),
          );
        }, ms(70)),
      );
      onTested?.();
    },
    [profile, reduced, onTested],
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

  return (
    <section className="screen" aria-labelledby="test-title">
      <p className="eyebrow">Setup · 7 of 8</p>
      <h1 className="screen-title" id="test-title">
        Test the board
      </h1>
      <p className="screen-lede">
        Press a physical key, or click one, and follow its route to the result.
        Numeric input is consumed while this screen is open.
      </p>

      <div className="board-stage board-stage--split">
        <NumpadBoard
          profile={profile}
          transient={transient}
          onPressKey={trigger}
          ariaLabel="Live test board"
        />
        <div className="board-side">
          {stages.length > 0 ? (
            <>
              <SignalTrace stages={stages} status={traceState} />
              <ExecutionReceipt receipt={receipt!} />
              <ActionRoute stages={stages} status={receipt?.status ?? "succeeded"} />
            </>
          ) : (
            <p className="prompt-note">
              The full route — key, action, adapter, result — appears here when
              a key fires.
            </p>
          )}
        </div>
      </div>
    </section>
  );
}
