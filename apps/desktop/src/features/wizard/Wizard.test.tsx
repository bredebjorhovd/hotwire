import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { Wizard } from "./Wizard";

describe("Wizard happy path", () => {
  it("walks all eight screens and finishes", async () => {
    const user = userEvent.setup();
    const onFinish = vi.fn();
    render(<Wizard onFinish={onFinish} />);

    // Screen 1 — Welcome
    expect(
      screen.getByRole("heading", { name: /more buttons/i }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Set up Hotwire" }));

    // Screen 2 — Select a control surface
    expect(
      screen.getByRole("heading", { name: /Which keys will drive Hotwire/i }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /numpad/i }));
    await user.click(screen.getByRole("button", { name: "Continue" }));

    // Screen 3 — Grant system permission
    await user.click(screen.getByRole("button", { name: "Open System Settings" }));
    expect(screen.getByText("Permission granted")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Continue" }));

    // Screen 4 — Capture hardware
    expect(screen.getByText("Press NUM 7")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Use standard layout" }));
    await user.click(screen.getByRole("button", { name: "Continue" }));

    // Screen 5 — Choose a starting profile
    await user.click(screen.getByRole("button", { name: /AI Numpad/i }));
    await user.click(screen.getByRole("button", { name: "Continue" }));

    // Screen 6 — Connect tools
    expect(
      screen.getByRole("heading", { name: /Connect your tools/i }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Continue" }));

    // Screen 7 — Test the board: press a key, see the full route
    expect(screen.getByRole("heading", { name: /Test the board/i })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /NUM 5 key/i }));
    const route = within(screen.getByRole("status", { name: "Action route" }));
    expect(route.getByText("NUM 5")).toBeInTheDocument();
    expect(route.getByText("OPEN OR FOCUS")).toBeInTheDocument();
    expect(route.getByText("HERDR")).toBeInTheDocument();
    expect(route.getByText("FOCUSED")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Continue" }));

    // Screen 8 — Done
    expect(
      screen.getByRole("heading", { name: /Your numpad is live/i }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Finish" }));

    expect(onFinish).toHaveBeenCalledTimes(1);
    expect(onFinish.mock.calls[0]?.[0]).toMatchObject({
      controlSurface: "numpad",
      permissionGranted: true,
      captureComplete: true,
      profileId: "ai-numpad",
    });
  });

  it("requires the permission step before continuing", async () => {
    const user = userEvent.setup();
    render(<Wizard onFinish={vi.fn()} />);

    await user.click(screen.getByRole("button", { name: "Set up Hotwire" }));
    await user.click(screen.getByRole("button", { name: /numpad/i }));
    await user.click(screen.getByRole("button", { name: "Continue" }));

    const continueButton = screen.getByRole("button", { name: "Continue" });
    expect(continueButton).toBeDisabled();

    await user.click(screen.getByRole("button", { name: "Open System Settings" }));
    expect(continueButton).toBeEnabled();
  });
});
