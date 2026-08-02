import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, within } from "@testing-library/react";

import { loadFixtureProfile } from "../../catalog/fixtures";
import { TestScreen } from "./TestScreen";

const profile = loadFixtureProfile("ai-numpad");

describe("TestScreen", () => {
  it("renders a full physical key → action → adapter → result route from a key press", () => {
    const onTested = vi.fn();
    render(<TestScreen profile={profile} onTested={onTested} />);

    fireEvent.keyDown(window, { code: "Numpad5" });

    const route = within(screen.getByRole("status", { name: "Action route" }));
    expect(route.getByText("NUM 5")).toBeInTheDocument();
    expect(route.getByText("OPEN OR FOCUS")).toBeInTheDocument();
    expect(route.getByText("HERDR")).toBeInTheDocument();
    expect(route.getByText("FOCUSED")).toBeInTheDocument();

    expect(onTested).toHaveBeenCalledTimes(1);
  });

  it("shows a route receipt for a hold binding (voice key)", () => {
    render(<TestScreen profile={profile} />);

    fireEvent.keyDown(window, { code: "Numpad0" });

    const route = within(screen.getByRole("status", { name: "Action route" }));
    expect(route.getByText("NUM 0")).toBeInTheDocument();
    expect(route.getByText("VOICE")).toBeInTheDocument();
    expect(route.getByText("PAPEGØYE")).toBeInTheDocument();
    expect(route.getByText("SHORTCUT SENT")).toBeInTheDocument();
  });
});
