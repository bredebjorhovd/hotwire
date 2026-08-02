import { useMemo, useState } from "react";

import type { Profile } from "@hotwire/schema";

import { Button } from "../../../components/Button";
import { actionShortLabel } from "../../catalog/actions";
import { bindingForCode, physicalKeyLabel } from "../../board/execution";

const summaryCodes = ["Numpad0", "Numpad5", "NumpadEnter"];

/** Screen 8 — Done (spec §4.1). */
export function DoneScreen({ profile }: { profile: Profile }) {
  const rows = useMemo(() => {
    const fromCodes = summaryCodes
      .map((code) => ({ code, binding: bindingForCode(profile, code) }))
      .filter((row) => row.binding)
      .map(({ code, binding }) => ({
        key: physicalKeyLabel(code),
        action:
          actionShortLabel(binding!.actionId) ?? binding!.actionId,
      }));
    if (fromCodes.length > 0) return fromCodes;
    return profile.bindings.slice(0, 3).map((binding) => ({
      key: physicalKeyLabel(binding.physicalCode),
      action: actionShortLabel(binding.actionId) ?? binding.actionId,
    }));
  }, [profile]);

  const [note, setNote] = useState<string | null>(null);

  return (
    <section className="screen screen--center" aria-labelledby="done-title">
      <p className="eyebrow">Setup complete</p>
      <h1 className="screen-title" id="done-title">
        Your numpad is live.
      </h1>
      <p className="screen-lede">
        {rows.length > 0
          ? `${profile.name} is ready. Here are its first bindings.`
          : `${profile.name} is ready — open the board to map your first key.`}
      </p>

      {rows.length > 0 && (
        <table className="summary-table">
          <tbody>
            {rows.map((row) => (
              <tr key={row.key}>
                <td>{row.key}</td>
                <td>{row.action}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      <div className="callout-actions" style={{ justifyContent: "center" }}>
        <Button
          variant="ghost"
          onClick={() => setNote("Keycap guide arrives with the icon-pack milestone.")}
        >
          Print keycap guide
        </Button>
        <Button
          variant="ghost"
          onClick={() => setNote("Icon-sheet export arrives with the icon-pack milestone.")}
        >
          Export icon sheet
        </Button>
      </div>
      {note && (
        <p className="prompt-note" role="status">
          {note}
        </p>
      )}
    </section>
  );
}
