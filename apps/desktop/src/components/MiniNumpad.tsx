import type { Profile } from "@hotwire/schema";

import { actionShortLabel } from "../features/catalog/actions";
import {
  bindingForCode,
  NUMPAD_LAYOUT,
  physicalKeySublabel,
} from "../features/board/execution";

/** Miniature mapped numpad shown on profile cards (spec §4.1 S5). */
export function MiniNumpad({ profile }: { profile: Profile }) {
  return (
    <div className="numpad numpad--compact" aria-hidden="true">
      {NUMPAD_LAYOUT.map(({ code, position, span }) => {
        const binding = bindingForCode(profile, code);
        return (
          <span
            key={code}
            className="mini-key"
            data-bound={binding ? "true" : "false"}
            data-key={position ? "Enter" : span ? "Numpad0" : code}
          >
            {binding
              ? (actionShortLabel(binding.actionId) ?? binding.actionId).slice(0, 3)
              : physicalKeySublabel(code)}
          </span>
        );
      })}
    </div>
  );
}
