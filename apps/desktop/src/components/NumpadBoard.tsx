import { useRef } from "react";

import type { Profile } from "@hotwire/schema";

import { actionShortLabel } from "../features/catalog/actions";
import {
  bindingForCode,
  NUMPAD_LAYOUT,
  numpadNeighbor,
  physicalKeyLabel,
  type FocusDirection,
} from "../features/board/execution";
import { VirtualKeycap, type KeycapState } from "./VirtualKeycap";

export interface NumpadBoardProps {
  profile: Profile;
  interactive?: boolean;
  /** Physical code put in the listening/capture state (spec §4.1 S4). */
  promptCode?: string;
  /** Physical codes that have settled during capture. */
  capturedCodes?: ReadonlySet<string>;
  /** Physical codes skipped during capture. */
  skippedCodes?: ReadonlySet<string>;
  /** Transient visual overrides keyed by physical code (pressed/triggered/failed). */
  transient?: Readonly<Record<string, KeycapState>>;
  /** Label overrides keyed by physical code (capture prompts). */
  labelOverrides?: Readonly<Record<string, string>>;
  onPressKey?: (code: string) => void;
  ariaLabel?: string;
}

function keyStateFor(
  code: string,
  props: NumpadBoardProps,
): KeycapState {
  const { promptCode, capturedCodes, skippedCodes, transient, profile } = props;
  if (transient?.[code]) return transient[code] as KeycapState;
  if (promptCode === code) return "listening";
  if (capturedCodes?.has(code)) return "settled";
  if (skippedCodes?.has(code)) return "unassigned";
  return bindingForCode(profile, code) ? "assigned" : "unassigned";
}

/**
 * The rendered numpad (spec §7.2 signature object). Pointer press and
 * arrow-key navigation are handled here; physical-key capture is wired by the
 * hosting screen.
 */
export function NumpadBoard({
  profile,
  interactive = true,
  promptCode,
  capturedCodes,
  skippedCodes,
  transient,
  labelOverrides,
  onPressKey,
  ariaLabel = "Interactive numpad",
}: NumpadBoardProps) {
  const keyRefs = useRef<Map<string, HTMLButtonElement>>(new Map());

  const moveFocus = (fromCode: string, direction: FocusDirection) => {
    const target = numpadNeighbor(fromCode, direction);
    if (target) {
      keyRefs.current.get(target)?.focus();
    }
  };

  return (
    <div className="board-tray" role="group" aria-label={ariaLabel}>
      <div className="numpad">
        {NUMPAD_LAYOUT.map(({ code, position, span }) => {
          const binding = bindingForCode(profile, code);
          const label = labelOverrides?.[code] ?? (binding
            ? actionShortLabel(binding.actionId) ?? binding.actionId
            : undefined);
          return (
            <VirtualKeycap
              key={code}
              code={code}
              label={label}
              state={keyStateFor(code, {
                profile,
                promptCode,
                capturedCodes,
                skippedCodes,
                transient,
              })}
              position={position}
              span={span}
              interactive={interactive}
              ariaLabel={`${physicalKeyLabel(code)} key${binding ? `, ${label}` : ""}`}
              onPress={onPressKey}
              onMoveFocus={moveFocus}
              ref={(node) => {
                if (node) keyRefs.current.set(code, node);
                else keyRefs.current.delete(code);
              }}
            />
          );
        })}
      </div>
    </div>
  );
}
