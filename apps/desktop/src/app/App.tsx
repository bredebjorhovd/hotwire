import { useEffect, useState } from "react";

import {
  getAppStatus,
  validateProfileYaml,
  type AppStatus,
  type ProfileValidationReport,
} from "../features/bridge/ipc";

const keys = [
  ["PROFILE", "CLAUDE", "CODEX", "VOICE"],
  ["NEW", "PLAN", "REVIEW", "ACCEPT"],
  ["TERMINAL", "HERDR", "TEST", "CONTINUE"],
  ["DIFF", "COMMIT", "PR", "EXECUTE"],
  ["VOICE", "VOICE", "REJECT", "EXECUTE"],
];

const exampleProfile = `
schemaVersion: 1
id: demo
name: Demo
controlSurface: numpad
bindings:
  - id: open-herdr
    physicalCode: Numpad5
    trigger: press
    actionId: app.open_or_focus
    adapterId: herdr
    consumeOriginal: true
    config: {}
`;

export function App() {
  const [active, setActive] = useState("HERDR");
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [validation, setValidation] = useState<ProfileValidationReport | null>(
    null,
  );

  useEffect(() => {
    void getAppStatus().then(setStatus);
  }, []);

  const runValidation = () => {
    setValidation(null);
    void validateProfileYaml(exampleProfile).then(setValidation);
  };

  return (
    <main className="shell">
      <header>
        <div className="brand">
          <span className="mark" /> HOTWIRE
        </div>
        <span className="status">
          <i /> Profile active
        </span>
      </header>

      <section className="hero">
        <p className="eyebrow">AI NUMPAD · LIVE BOARD</p>
        <h1>
          Your keyboard has more buttons
          <br />
          than your workflow needs.
        </h1>
        <p className="lede">Press a key to trace its route through Hotwire.</p>
      </section>

      <section className="board" aria-label="Interactive numpad prototype">
        {keys.flatMap((row, rowIndex) =>
          row.map((label, columnIndex) => {
            const id = `${rowIndex}-${columnIndex}`;
            return (
              <button
                className={active === label ? "key active" : "key"}
                key={id}
                onClick={() => setActive(label)}
              >
                <span>{label === "HERDR" ? "5" : label === "VOICE" ? "0" : "·"}</span>
                <strong>{label}</strong>
              </button>
            );
          }),
        )}
      </section>

      <section className="trace" aria-live="polite">
        <span>PHYSICAL KEY</span>
        <b>→</b>
        <span>{active}</span>
        <b>→</b>
        <span>{active === "HERDR" ? "LAUNCH OR FOCUS" : "SEMANTIC ACTION"}</span>
        <em>READY</em>
      </section>

      <section className="system" aria-label="Desktop shell status">
        <span className="system-item">
          SHELL
          <b>{status?.appVersion ?? "…"}</b>
        </span>
        <span className="system-item">
          SCHEMA
          <b>v{status?.profileSchemaVersion ?? 1}</b>
        </span>
        <span className="system-item">
          INPUT
          <b>{status?.inputBackend ?? "none"}</b>
        </span>
        <button className="system-check" onClick={runValidation}>
          VALIDATE PROFILE
        </button>
        {validation && (
          <span
            className={
              validation.valid ? "system-result ok" : "system-result bad"
            }
          >
            {validation.valid ? "PROFILE VALID" : "PROFILE INVALID"}
          </span>
        )}
      </section>
    </main>
  );
}
