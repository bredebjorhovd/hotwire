import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";

import { SignalTrace } from "./SignalTrace";

const stages = [
  { label: "NUM 5", detail: "Physical key" },
  { label: "OPEN OR FOCUS", detail: "Action" },
  { label: "HERDR", detail: "Adapter" },
  { label: "FOCUSED", detail: "Result" },
];

describe("SignalTrace", () => {
  it("renders each stage of the route", () => {
    render(<SignalTrace stages={stages} status="done" />);

    expect(screen.getByText("NUM 5")).toBeInTheDocument();
    expect(screen.getByText("OPEN OR FOCUS")).toBeInTheDocument();
    expect(screen.getByText("HERDR")).toBeInTheDocument();
    expect(screen.getByText("FOCUSED")).toBeInTheDocument();
    expect(screen.getByText("Physical key")).toBeInTheDocument();
    expect(screen.getByText("Result")).toBeInTheDocument();
  });

  it("marks the final node as failed when the route fails", () => {
    render(<SignalTrace stages={stages} status="failed" />);
    const nodes = screen.getAllByRole("listitem");
    const last = nodes[nodes.length - 1];
    expect(last?.querySelector(".trace-node")).toHaveAttribute(
      "data-state",
      "failed",
    );
  });
});
