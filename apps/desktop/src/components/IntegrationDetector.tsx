import { useState } from "react";

import { integrations, type IntegrationStatus } from "../features/catalog/integrations";

function badgeFor(status: IntegrationStatus): string {
  switch (status) {
    case "found":
    case "found-path":
      return "found";
    case "running":
      return "running";
    case "missing":
      return "missing";
  }
}

/**
 * Detected-tools list for the Connect screen (spec §4.1 S6). Each row has a
 * status badge and a Configure disclosure; absence is never treated as failure.
 */
export function IntegrationDetector() {
  const [open, setOpen] = useState<string | null>(null);

  return (
    <ul className="integration-list" role="list" aria-label="Detected tools">
      {integrations.map((item) => (
        <li
          key={item.id}
          className="integration-row"
          data-open={open === item.id ? "true" : "false"}
        >
          <button
            type="button"
            className="integration-summary"
            aria-expanded={open === item.id}
            onClick={() => setOpen(open === item.id ? null : item.id)}
          >
            <span className="integration-icon" aria-hidden="true">
              {item.name.charAt(0)}
            </span>
            <span className="integration-name">
              <b>{item.name}</b>
              <span>{item.detail}</span>
            </span>
            <span className={`badge badge--${badgeFor(item.status)}`}>
              {item.note}
            </span>
            <span className="badge" aria-hidden="true">
              Configure
            </span>
          </button>
          <div className="integration-detail" role="region">
            <p>{item.disclosure}</p>
          </div>
        </li>
      ))}
    </ul>
  );
}
