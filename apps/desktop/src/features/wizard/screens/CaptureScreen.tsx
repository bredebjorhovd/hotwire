import { useCallback, useEffect, useState } from "react";

import { Button } from "../../../components/Button";
import { NumpadBoard } from "../../../components/NumpadBoard";
import { physicalKeyLabel } from "../../board/execution";
import { loadFixtureProfile } from "../../catalog/fixtures";

/** Keys the wizard captures to distinguish the keyboard and Num Lock behavior. */
export const CAPTURE_SEQUENCE = [
  "Numpad7",
  "Numpad1",
  "NumpadEnter",
  "NumLock",
];

export interface CaptureScreenProps {
  onComplete: (complete: boolean) => void;
}

/**
 * Screen 4 — Capture hardware (spec §4.1). The virtual numpad pulses one key
 * at a time; press it (physical key or click) and it settles into place.
 */
export function CaptureScreen({ onComplete }: CaptureScreenProps) {
  const [index, setIndex] = useState(0);
  const [captured, setCaptured] = useState<ReadonlySet<string>>(new Set());
  const [skipped, setSkipped] = useState<ReadonlySet<string>>(new Set());
  const [standard, setStandard] = useState(false);

  const done = standard || index >= CAPTURE_SEQUENCE.length;
  const current =
    index < CAPTURE_SEQUENCE.length ? CAPTURE_SEQUENCE[index] : null;

  useEffect(() => {
    onComplete(done);
  }, [done, onComplete]);

  const capture = useCallback((code: string) => {
    setCaptured((prev) => new Set(prev).add(code));
    setIndex((i) => i + 1);
  }, []);

  const skip = () => {
    if (current) setSkipped((prev) => new Set(prev).add(current));
    setIndex((i) => i + 1);
  };

  const reset = () => {
    setIndex(0);
    setCaptured(new Set());
    setSkipped(new Set());
    setStandard(false);
  };

  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if (event.repeat) return;
      if (current && event.code === current) {
        event.preventDefault();
        capture(event.code);
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [current, capture]);

  const labelOverrides: Record<string, string> = {};
  for (const code of CAPTURE_SEQUENCE) {
    if (captured.has(code)) labelOverrides[code] = "OK";
    if (skipped.has(code)) labelOverrides[code] = "SKIP";
  }
  if (current) labelOverrides[current] = "PRESS";

  const blank = loadFixtureProfile("blank");

  return (
    <section className="screen" aria-labelledby="capture-title">
      <p className="eyebrow">Setup · 4 of 8</p>
      <h1 className="screen-title" id="capture-title">
        Capture your hardware
      </h1>
      <p className="screen-lede">
        Press the pulsing key so Hotwire can tell your keyboard apart and read
        Num Lock correctly.
      </p>

      <div className="capture-stage">
        <NumpadBoard
          profile={blank}
          promptCode={current ?? undefined}
          capturedCodes={captured}
          skippedCodes={skipped}
          labelOverrides={labelOverrides}
          onPressKey={(code) => {
            if (code === current) capture(code);
          }}
          ariaLabel="Hardware capture numpad"
        />
        <div className="capture-prompt" aria-live="polite">
          <p className="capture-count">
            KEY {Math.min(index + 1, CAPTURE_SEQUENCE.length)} /{" "}
            {CAPTURE_SEQUENCE.length}
          </p>
          {current ? (
            <>
              <p className="prompt-key">Press {physicalKeyLabel(current)}</p>
              <p className="prompt-note">
                The key settles into place with a mechanical response when it
                lands.
              </p>
            </>
          ) : (
            <>
              <p className="prompt-key">All keys captured.</p>
              <p className="prompt-note">
                {captured.size} key{captured.size === 1 ? "" : "s"} captured
                {skipped.size > 0 ? `, ${skipped.size} skipped` : ""}.
              </p>
            </>
          )}
          <div className="callout-actions">
            <Button variant="primary" onClick={() => setStandard(true)}>
              Use standard layout
            </Button>
            <Button onClick={skip} disabled={!current}>
              Skip unavailable key
            </Button>
            <Button variant="ghost" onClick={reset}>
              Start over
            </Button>
          </div>
        </div>
      </div>
    </section>
  );
}
