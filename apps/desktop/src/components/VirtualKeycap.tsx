import { forwardRef, type KeyboardEvent } from "react";

import {
  physicalKeyLabel,
  physicalKeySublabel,
  type FocusDirection,
} from "../features/board/execution";

export type KeycapState =
  | "unassigned"
  | "assigned"
  | "hovered"
  | "pressed"
  | "triggered"
  | "failed"
  | "listening"
  | "settled";

export interface VirtualKeycapProps {
  code: string;
  /** Primary legend on the key top (the semantic action label). */
  label?: string;
  /** Small engraved legend (usually the physical digit). */
  sublabel?: string;
  state?: KeycapState;
  /** Marks the Enter key as spanning two rows in the numpad grid. */
  position?: "row3";
  /** Marks the wide zero as spanning two columns. */
  span?: 1 | 2;
  ariaLabel?: string;
  interactive?: boolean;
  tabIndex?: number;
  onPress?: (code: string) => void;
  onMoveFocus?: (code: string, direction: FocusDirection) => void;
}

/**
 * The tactile signature keycap (spec §7.2): visible top surface, darker front
 * plane, inner highlight, soft contact shadow, material grain and plausible
 * travel. State drives the visual (hovered / pressed / triggered / failed /
 * listening), all keyboard-navigable.
 */
export const VirtualKeycap = forwardRef<HTMLButtonElement, VirtualKeycapProps>(
  function VirtualKeycap(
    {
      code,
      label,
      sublabel,
      state = "assigned",
      position,
      span,
      ariaLabel,
      interactive = true,
      tabIndex = 0,
      onPress,
      onMoveFocus,
    },
    ref,
  ) {
    const pressed =
      state === "pressed" ||
      state === "triggered" ||
      state === "failed";

    const handleKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
      if (!onMoveFocus) return;
      const direction: FocusDirection | null =
        event.key === "ArrowUp"
          ? "up"
          : event.key === "ArrowDown"
            ? "down"
            : event.key === "ArrowLeft"
              ? "left"
              : event.key === "ArrowRight"
                ? "right"
                : null;
      if (direction) {
        event.preventDefault();
        onMoveFocus(code, direction);
      }
    };

    return (
      <button
        ref={ref}
        type="button"
        className="keycap"
        data-state={state}
        data-position={position}
        data-span={span}
        aria-label={ariaLabel ?? `${physicalKeyLabel(code)} key`}
        aria-pressed={pressed}
        tabIndex={interactive ? tabIndex : -1}
        onClick={interactive ? () => onPress?.(code) : undefined}
        onKeyDown={handleKeyDown}
      >
        <span className="key-label">{label ?? "—"}</span>
        <span className="key-sublabel">{sublabel ?? physicalKeySublabel(code)}</span>
      </button>
    );
  },
);
