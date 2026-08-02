/**
 * Fixture integration-detection results for the Connect tools screen (spec
 * §4.1 S6). Absence is never treated as failure — every row offers a configure
 * disclosure and a generic fallback.
 */

export type IntegrationStatus =
  | "found"
  | "found-path"
  | "running"
  | "missing";

export interface IntegrationFixture {
  id: string;
  name: string;
  detail: string;
  status: IntegrationStatus;
  note: string;
  disclosure: string;
}

export const integrations: IntegrationFixture[] = [
  {
    id: "herdr",
    name: "Herdr",
    detail: "herdr 0.4.0",
    status: "found",
    note: "Found",
    disclosure: "Focus and agent actions route through the local Herdr API. No account or auth is required inside Hotwire.",
  },
  {
    id: "claude-code",
    name: "Claude Code",
    detail: "claude — /opt/homebrew/bin/claude",
    status: "found-path",
    note: "Found in PATH",
    disclosure: "Launches Claude Code in your configured terminal and hands over the reusable prompt. Fallback: a generic keyboard shortcut.",
  },
  {
    id: "codex",
    name: "Codex",
    detail: "codex — /opt/homebrew/bin/codex",
    status: "found-path",
    note: "Found in PATH",
    disclosure: "Launches Codex CLI in your configured terminal. Fallback: a generic keyboard shortcut.",
  },
  {
    id: "papegoye",
    name: "Papegøye",
    detail: "Papegøye 1.2.0",
    status: "running",
    note: "Running",
    disclosure: "Voice keys hold the configured push-to-talk shortcut. Audio stays in Papegøye — Hotwire never captures microphone input.",
  },
  {
    id: "terminal",
    name: "Terminal",
    detail: "iTerm2",
    status: "found",
    note: "iTerm",
    disclosure: "Terminal and test actions open iTerm2 in the current project directory.",
  },
  {
    id: "git",
    name: "Git",
    detail: "git 2.46.0",
    status: "found",
    note: "Found",
    disclosure: "Diff, commit and pull-request actions run against the current repository. Commands are inspectable before execution.",
  },
];
