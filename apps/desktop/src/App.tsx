import { useState } from "react";

const keys = [
  ["PROFILE", "CLAUDE", "CODEX", "VOICE"],
  ["NEW", "PLAN", "REVIEW", "ACCEPT"],
  ["TERMINAL", "HERDR", "TEST", "CONTINUE"],
  ["DIFF", "COMMIT", "PR", "EXECUTE"],
  ["VOICE", "VOICE", "REJECT", "EXECUTE"],
];

export function App() {
  const [active, setActive] = useState("HERDR");

  return (
    <main className="shell">
      <header>
        <div className="brand"><span className="mark" /> HOTWIRE</div>
        <span className="status"><i /> Profile active</span>
      </header>

      <section className="hero">
        <p className="eyebrow">AI NUMPAD · LIVE BOARD</p>
        <h1>Your keyboard has more buttons<br />than your workflow needs.</h1>
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
        <span>PHYSICAL KEY</span><b>→</b><span>{active}</span><b>→</b>
        <span>{active === "HERDR" ? "LAUNCH OR FOCUS" : "SEMANTIC ACTION"}</span>
        <em>READY</em>
      </section>
    </main>
  );
}

