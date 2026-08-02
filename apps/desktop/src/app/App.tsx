import { useEffect, useState } from "react";

import { Button } from "../components/Button";
import { loadFixtureProfile } from "../features/catalog/fixtures";
import { useTheme } from "../features/board/useTheme";
import { BoardScreen } from "../features/board/BoardScreen";
import { Wizard, type WizardChoices } from "../features/wizard/Wizard";
import {
  emitMockActionReceipt,
  getAppStatus,
  isRunningInTauri,
  type AppStatus,
} from "../features/bridge/ipc";
import { receiptRouteLabel } from "../features/bridge/receipts";
import { useActionReceipts } from "../features/bridge/useActionReceipts";

/**
 * Application root. First launch opens directly into the setup wizard; after
 * Finish it shows the live Board. Dark/light appearance and the shell-status
 * footer live here so every screen inherits them. The footer's LAST ACTION
 * item mirrors the menu-bar popover (§6.1) and is fed by native
 * `ActionReceipt` events when running under Tauri.
 */
export function App() {
  const { theme, toggle } = useTheme();
  const [mode, setMode] = useState<"wizard" | "board">("wizard");
  const [activeProfileId, setActiveProfileId] = useState("ai-numpad");
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [sendingMock, setSendingMock] = useState(false);
  const receipt = useActionReceipts();

  useEffect(() => {
    void getAppStatus().then(setStatus);
  }, []);

  const finishSetup = (choices: WizardChoices) => {
    if (choices.profileId) setActiveProfileId(choices.profileId);
    setMode("board");
  };

  const sendMockReceipt = async () => {
    setSendingMock(true);
    try {
      await emitMockActionReceipt();
    } finally {
      setSendingMock(false);
    }
  };

  const activeProfile = loadFixtureProfile(activeProfileId);

  return (
    <div className="app-shell">
      <header className="app-header">
        <span className="brand">
          <span className="mark" aria-hidden="true" />
          HOTWIRE
        </span>
        <span className="header-tools">
          {isRunningInTauri() && (
            <Button
              variant="ghost"
              onClick={sendMockReceipt}
              disabled={sendingMock}
              aria-label="Emit a mocked action receipt from the Rust shell"
            >
              TEST RECEIPT
            </Button>
          )}
          <Button
            variant="ghost"
            onClick={toggle}
            aria-label={`Switch to ${theme === "dark" ? "light" : "dark"} appearance`}
          >
            {theme === "dark" ? "LIGHT" : "DARK"}
          </Button>
        </span>
      </header>

      {mode === "wizard" ? (
        <Wizard onFinish={finishSetup} />
      ) : (
        <BoardScreen profile={activeProfile} />
      )}

      <footer className="app-footer">
        <div className="system-strip" aria-label="Desktop shell status">
          <span className="system-item">
            SHELL <b>{status?.appVersion ?? "…"}</b>
          </span>
          <span className="system-item">
            SCHEMA <b>v{status?.profileSchemaVersion ?? 1}</b>
          </span>
          <span className="system-item">
            INPUT <b>{status?.inputBackend ?? "none"}</b>
          </span>
          <span className="system-item">
            CAPTURE <b>{status?.captureAvailable ? "on" : "off"}</b>
          </span>
          <span className="system-item" aria-label="Last action">
            LAST ACTION{" "}
            <b>{receipt ? receiptRouteLabel(receipt) : "—"}</b>
          </span>
        </div>
      </footer>
    </div>
  );
}
