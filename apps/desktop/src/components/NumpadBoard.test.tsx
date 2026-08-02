import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { loadFixtureProfile } from "../features/catalog/fixtures";
import { NumpadBoard } from "./NumpadBoard";

const profile = loadFixtureProfile("ai-numpad");

describe("NumpadBoard", () => {
  it("renders every numpad key with its binding label", () => {
    render(<NumpadBoard profile={profile} />);
    expect(screen.getByRole("button", { name: /NUM 5 key/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /NUM 5 key/i })).toHaveTextContent(
      "OPEN OR FOCUS",
    );
    expect(screen.getByRole("button", { name: /ENTER key/i })).toBeInTheDocument();
  });

  it("responds to pointer input and reports the pressed code", async () => {
    const user = userEvent.setup();
    const onPress = vi.fn();
    render(<NumpadBoard profile={profile} onPressKey={onPress} />);

    await user.click(screen.getByRole("button", { name: /NUM 5 key/i }));
    expect(onPress).toHaveBeenCalledWith("Numpad5");
  });

  it("moves focus with arrow keys and activates with Enter", async () => {
    const user = userEvent.setup();
    const onPress = vi.fn();
    render(<NumpadBoard profile={profile} onPressKey={onPress} />);

    const num5 = screen.getByRole("button", { name: /NUM 5 key/i });
    num5.focus();

    await user.keyboard("{ArrowUp}");
    expect(screen.getByRole("button", { name: /NUM 8 key/i })).toHaveFocus();

    await user.keyboard("{ArrowLeft}");
    expect(screen.getByRole("button", { name: /NUM 7 key/i })).toHaveFocus();

    await user.keyboard("{Enter}");
    expect(onPress).toHaveBeenCalledWith("Numpad7");
  });
});
