import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { App } from "./App";

describe("App", () => {
  it("opens directly into the wizard with theme toggle and shell status", async () => {
    const user = userEvent.setup();
    render(<App />);

    // First launch opens into the wizard (spec §4.1), no dashboard first.
    expect(
      screen.getByRole("heading", { name: /more buttons/i }),
    ).toBeInTheDocument();

    // Theme toggle flips between light and dark appearances.
    await user.click(screen.getByRole("button", { name: /switch to light/i }));
    expect(
      screen.getByRole("button", { name: /switch to dark/i }),
    ).toBeInTheDocument();

    // Shell-status footer mirrors the BOOT-001 IPC boundary.
    expect(screen.getByText("SHELL")).toBeInTheDocument();
    expect(screen.getByText("SCHEMA")).toBeInTheDocument();

    // The menu-bar popover's "last action" readout (spec §6.1) renders empty
    // until the shell emits a receipt; outside Tauri no mock button appears.
    expect(screen.getByText("LAST ACTION")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /emit a mocked action receipt/i }),
    ).not.toBeInTheDocument();
  });
});
