import { IntegrationDetector } from "../../../components/IntegrationDetector";

/** Screen 6 — Connect tools (spec §4.1). */
export function ConnectScreen() {
  return (
    <section className="screen" aria-labelledby="connect-title">
      <p className="eyebrow">Setup · 6 of 8</p>
      <h1 className="screen-title" id="connect-title">
        Connect your tools
      </h1>
      <p className="screen-lede">
        Hotwire detects the tools already on this machine. Nothing here requires
        an account — missing tools fall back to generic shortcuts.
      </p>
      <IntegrationDetector />
    </section>
  );
}
