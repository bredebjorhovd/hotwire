/**
 * Typed bridge between the React frontend and the Rust desktop shell.
 *
 * Every function here mirrors a `#[tauri::command]` in
 * `apps/desktop/src-tauri/src/commands.rs`. Outside the Tauri runtime (plain
 * `vite dev` in a browser) the bridge degrades gracefully so the interaction
 * prototype keeps working without the shell.
 */

import { invoke } from "@tauri-apps/api/core";

import { parseProfileYaml } from "@hotwire/profiles";

/** Rust `AppStatus` (see `commands.rs`), serialized with camelCase. */
export interface AppStatus {
  appVersion: string;
  profileSchemaVersion: number;
  inputBackend: string;
  captureAvailable: boolean;
}

/** Rust `ProfileValidationReport` (see `commands.rs`). */
export interface ProfileValidationReport {
  valid: boolean;
  profile?: unknown;
  error?: string;
}

/** Whether the page is running inside the Tauri webview. */
export function isRunningInTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/**
 * Reports the desktop shell status.
 *
 * In the browser this returns a preview value so the prototype renders
 * without a Rust backend.
 */
export async function getAppStatus(): Promise<AppStatus> {
  if (!isRunningInTauri()) {
    return {
      appVersion: "browser-preview",
      profileSchemaVersion: 1,
      inputBackend: "none",
      captureAvailable: false,
    };
  }
  return invoke<AppStatus>("app_status");
}

/**
 * Validates a YAML profile document.
 *
 * Under Tauri this runs the `hotwire-profile` validation boundary in Rust;
 * in the browser it falls back to the shared `@hotwire/profiles` validator so
 * both sides stay exercised.
 */
export async function validateProfileYaml(
  yaml: string,
): Promise<ProfileValidationReport> {
  if (!isRunningInTauri()) {
    const result = parseProfileYaml(yaml);
    return result.ok
      ? { valid: true, profile: result.profile }
      : { valid: false, error: result.error };
  }
  return invoke<ProfileValidationReport>("validate_profile", { yaml });
}
