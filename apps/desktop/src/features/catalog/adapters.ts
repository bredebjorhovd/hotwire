/**
 * Fixture adapter manifests.
 *
 * Platform-neutral `AdapterManifest` records (spec §12.2) describing the
 * first-party adapters the prototype can route through.
 */

import type { AdapterManifest } from "@hotwire/schema";

export const adapters: AdapterManifest[] = [
  {
    id: "herdr",
    name: "Herdr",
    version: "0.1.0",
    icon: "herdr",
    capabilities: ["focus", "new_task", "continue", "review", "accept"],
    configSchema: {},
  },
  {
    id: "papegoye",
    name: "Papegøye",
    version: "0.1.0",
    icon: "papegoye",
    capabilities: ["start", "stop", "cancel"],
    configSchema: {},
  },
  {
    id: "claude-code",
    name: "Claude Code",
    version: "0.1.0",
    icon: "claude",
    capabilities: ["launch", "prompt"],
    configSchema: {},
  },
  {
    id: "codex",
    name: "Codex",
    version: "0.1.0",
    icon: "codex",
    capabilities: ["launch", "prompt"],
    configSchema: {},
  },
  {
    id: "terminal",
    name: "Terminal",
    version: "0.1.0",
    icon: "terminal",
    capabilities: ["open", "run"],
    configSchema: {},
  },
  {
    id: "app",
    name: "Application",
    version: "0.1.0",
    icon: "app",
    capabilities: ["open_or_focus"],
    configSchema: {},
  },
  {
    id: "shortcut",
    name: "Shortcut",
    version: "0.1.0",
    icon: "shortcut",
    capabilities: ["send"],
    configSchema: {},
  },
  {
    id: "git",
    name: "Git",
    version: "0.1.0",
    icon: "git",
    capabilities: ["diff", "commit", "pr"],
    configSchema: {},
  },
];

export const adapterIndex = new Map(adapters.map((a) => [a.id, a]));

export function getAdapter(id: string): AdapterManifest | undefined {
  return adapterIndex.get(id);
}
