import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// @testing-library/react only auto-registers cleanup when vitest globals are
// enabled; register it explicitly here (vitest runs with `globals: false`).
afterEach(() => {
  cleanup();
});
