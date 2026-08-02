/**
 * Fixture semantic-action catalog.
 *
 * Platform-neutral `ActionDefinition` records (spec §12.1) used by the
 * Milestone 0 prototype. These are demo data — no native integration is
 * implied until the corresponding adapter lands.
 */

import type { ActionDefinition } from "@hotwire/schema";

export const actions: ActionDefinition[] = [
  {
    id: "app.open_or_focus",
    label: "Open or focus",
    description: "Launch or focus the target application",
    icon: "focus",
    category: "AI tools",
    risk: "none",
    supportedTriggers: ["press"],
  },
  {
    id: "voice.input",
    label: "Voice input",
    description: "Start Papegøye dictation",
    icon: "voice",
    category: "Voice",
    risk: "none",
    supportedTriggers: ["press", "hold"],
  },
  {
    id: "agent.new",
    label: "New agent",
    description: "Start a new coding agent",
    icon: "new",
    category: "AI tools",
    risk: "none",
    supportedTriggers: ["press"],
  },
  {
    id: "agent.continue",
    label: "Continue",
    description: "Continue the active agent",
    icon: "continue",
    category: "AI tools",
    risk: "none",
    supportedTriggers: ["press"],
  },
  {
    id: "agent.plan",
    label: "Plan",
    description: "Ask the agent to plan the change",
    icon: "plan",
    category: "AI tools",
    risk: "none",
    supportedTriggers: ["press"],
  },
  {
    id: "agent.review",
    label: "Review",
    description: "Review the current change",
    icon: "review",
    category: "AI tools",
    risk: "none",
    supportedTriggers: ["press"],
  },
  {
    id: "agent.accept",
    label: "Accept",
    description: "Accept the agent's proposal",
    icon: "accept",
    category: "AI tools",
    risk: "none",
    supportedTriggers: ["press"],
  },
  {
    id: "agent.reject",
    label: "Reject",
    description: "Reject the agent's proposal",
    icon: "reject",
    category: "AI tools",
    risk: "none",
    supportedTriggers: ["press"],
  },
  {
    id: "test.run",
    label: "Run tests",
    description: "Run the test suite in the terminal",
    icon: "test",
    category: "Terminal and scripts",
    risk: "low",
    supportedTriggers: ["press"],
  },
  {
    id: "terminal.open",
    label: "Open terminal",
    description: "Open a terminal in the project",
    icon: "terminal",
    category: "Terminal and scripts",
    risk: "none",
    supportedTriggers: ["press"],
  },
  {
    id: "git.diff",
    label: "Show diff",
    description: "Show the current diff",
    icon: "diff",
    category: "Terminal and scripts",
    risk: "none",
    supportedTriggers: ["press"],
  },
  {
    id: "git.commit",
    label: "Commit",
    description: "Commit staged changes",
    icon: "commit",
    category: "Terminal and scripts",
    risk: "low",
    supportedTriggers: ["press"],
  },
  {
    id: "git.pr",
    label: "Open pull request",
    description: "Open a pull request for the branch",
    icon: "pr",
    category: "Terminal and scripts",
    risk: "low",
    supportedTriggers: ["press"],
  },
  {
    id: "claude.launch",
    label: "Launch Claude Code",
    description: "Open Claude Code in a terminal",
    icon: "claude",
    category: "AI tools",
    risk: "none",
    supportedTriggers: ["press"],
  },
  {
    id: "codex.launch",
    label: "Launch Codex",
    description: "Open Codex in a terminal",
    icon: "codex",
    category: "AI tools",
    risk: "none",
    supportedTriggers: ["press"],
  },
  {
    id: "profile.switch",
    label: "Switch profile",
    description: "Cycle the active profile",
    icon: "profile",
    category: "System",
    risk: "none",
    supportedTriggers: ["press"],
  },
];

const shortLabels: Record<string, string> = {
  "app.open_or_focus": "OPEN OR FOCUS",
  "voice.input": "VOICE",
  "agent.new": "NEW AGENT",
  "agent.continue": "CONTINUE",
  "agent.plan": "PLAN",
  "agent.review": "REVIEW",
  "agent.accept": "ACCEPT",
  "agent.reject": "REJECT",
  "test.run": "RUN TESTS",
  "terminal.open": "TERMINAL",
  "git.diff": "DIFF",
  "git.commit": "COMMIT",
  "git.pr": "PULL REQUEST",
  "claude.launch": "CLAUDE",
  "codex.launch": "CODEX",
  "profile.switch": "PROFILE",
};

export const actionIndex = new Map(actions.map((a) => [a.id, a]));

export function getAction(id: string): ActionDefinition | undefined {
  return actionIndex.get(id);
}

/** Uppercase legend used on keycaps and in route receipts (spec §4.1 S8). */
export function actionShortLabel(id: string): string | undefined {
  return shortLabels[id];
}
