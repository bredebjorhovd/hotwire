import { useState } from "react";

import { Button } from "../../../components/Button";

const statements = [
  "Hotwire listens only to keys required for active bindings.",
  "No typed text is stored.",
  "Input events are processed locally.",
  "Permission can be revoked at any time.",
];

export interface PermissionScreenProps {
  granted: boolean;
  onGrant: () => void;
}

/**
 * Screen 3 — Grant system permission (spec §4.1). The browser prototype
 * simulates opening System Settings; the native shell will surface the real
 * Accessibility/Input Monitoring state over the IPC bridge.
 */
export function PermissionScreen({ granted, onGrant }: PermissionScreenProps) {
  const [settingsOpened, setSettingsOpened] = useState(granted);

  const openSettings = () => {
    setSettingsOpened(true);
    onGrant();
  };

  const retry = () => {
    if (settingsOpened) onGrant();
  };

  return (
    <section className="screen" aria-labelledby="permission-title">
      <p className="eyebrow">Setup · 3 of 8</p>
      <h1 className="screen-title" id="permission-title">
        Grant Hotwire system permission
      </h1>
      <p className="screen-lede">
        To turn physical keys into a control surface, macOS needs to let
        Hotwire observe the keyboard. Capture stays off until you grant it.
      </p>

      <div className="callout">
        <h3>What this permission means</h3>
        <ul className="callout-list">
          {statements.map((statement) => (
            <li key={statement}>
              <span className="tick" aria-hidden="true">
                ✓
              </span>
              <span>{statement}</span>
            </li>
          ))}
        </ul>

        <span className="permission-state" data-state={granted ? "granted" : "denied"}>
          {granted ? "Permission granted" : "Waiting for permission"}
        </span>

        <div className="callout-actions">
          <Button variant="primary" onClick={openSettings}>
            Open System Settings
          </Button>
          <Button onClick={retry}>Check again</Button>
        </div>
        <p className="prompt-note">
          {granted
            ? "Capture is armed. You can revoke this at any time in System Settings."
            : "Prototype note: in this preview, opening System Settings simulates granting permission."}
        </p>
      </div>
    </section>
  );
}
