/**
 * Typed bridge between the React frontend and the Rust desktop shell.
 *
 * Every function here mirrors a `#[tauri::command]` or event in
 * `apps/desktop/src-tauri/src/` (`commands.rs`, `events.rs`). Outside the
 * Tauri runtime (plain `vite dev` in a browser) the bridge degrades
 * gracefully so the interaction prototype keeps working without the shell.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import { parseProfileYaml } from "@hotwire/profiles";

/** Rust `AppStatus` (see `commands.rs`), serialized with camelCase. */
export interface AppStatus {
  appVersion: string;
  profileSchemaVersion: number;
  inputBackend: string;
  captureAvailable: boolean;
}

/** Rust `PermissionStatus` (see `crates/hotwire-core`). */
export type PermissionStatus = "authorized" | "denied";

/** Rust `CaptureStatus` (see `crates/hotwire-core`). */
export type CaptureStatus =
  | "stopped"
  | "running"
  | "disabled_by_timeout"
  | "disabled_by_user_input"
  | "start_failed";

/** Rust `CaptureHealth` (see `crates/hotwire-core`). */
export interface CaptureHealth {
  permission: PermissionStatus;
  status: CaptureStatus;
  paused: boolean;
}

/** Rust `ActionSummary` (see `crates/hotwire-core`). */
export interface ActionSummary {
  actionId: string;
  adapterId: string;
  status: ActionReceiptStatus;
}

/**
 * Rust `DiagnosticsReport` (see `commands.rs` / `crates/hotwire-core`).
 *
 * Deliberately restricted to permitted categories; it never carries typed
 * text, prompts, exact commands, or arbitrary key sequences (spec §21).
 */
export interface DiagnosticsReport {
  appVersion?: string | null;
  capture: CaptureHealth;
  lastAction?: ActionSummary | null;
}

/** Rust `ProfileValidationReport` (see `commands.rs`). */
export interface ProfileValidationReport {
  valid: boolean;
  profile?: unknown;
  error?: string;
}

/** Rust `ActionStatus` (see `crates/hotwire-core`), serialized snake_case. */
export type ActionReceiptStatus = "started" | "succeeded" | "failed" | "cancelled";

/** Rust `ActionReceipt` (see `crates/hotwire-core`), serialized camelCase. */
export interface ActionReceipt {
  executionId: string;
  profileId: string;
  bindingId: string;
  physicalCode: string;
  actionId: string;
  adapterId: string;
  status: ActionReceiptStatus;
  message?: string | null;
}

export interface PendingReview {
  id: string;
  command: string;
}

export async function getPendingReviews(): Promise<PendingReview[]> {
  if (!isRunningInTauri()) return [];
  return invoke<PendingReview[]>("pending_reviews");
}

export async function approveReview(id: string): Promise<void> {
  if (isRunningInTauri()) await invoke("approve_review", { id });
}

export async function denyReview(id: string): Promise<void> {
  if (isRunningInTauri()) await invoke("deny_review", { id });
}

/**
 * Event name for `ActionReceipt` payloads.
 *
 * Mirrors `ACTION_RECEIPT_EVENT` in `apps/desktop/src-tauri/src/events.rs`.
 */
export const ACTION_RECEIPT_EVENT = "action-receipt";

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

/** Activates the selected profile in the native event router. */
export async function activateProfileYaml(yaml: string): Promise<void> {
  if (isRunningInTauri()) {
    await invoke("activate_profile", { yaml });
  }
}

/**
 * Reveals and focuses the configuration window (menu-bar "Open Hotwire…").
 *
 * Browser no-op, so the web preview stays identical.
 */
export async function showMainWindow(): Promise<void> {
  if (isRunningInTauri()) {
    await invoke("show_main_window");
  }
}

/**
 * Quits the desktop shell.
 *
 * Browser no-op.
 */
export async function quitDesktop(): Promise<void> {
  if (isRunningInTauri()) {
    await invoke("quit");
  }
}

/**
 * Asks the shell to emit a mocked `ActionReceipt` event.
 *
 * Returns the receipt for callers that want it without listening. Browser
 * no-op returning `null`, so the web preview never fakes an event it could
 * not have delivered.
 */
export async function emitMockActionReceipt(): Promise<ActionReceipt | null> {
  if (!isRunningInTauri()) return null;
  return invoke<ActionReceipt>("mock_action_receipt");
}

/**
 * Returns a diagnostics snapshot (spec §6.4).
 *
 * In the browser this returns a safe preview so the prototype renders without
 * a Rust backend; the report never contains typed text, prompts, exact
 * commands, or arbitrary key sequences.
 */
export async function getDiagnostics(): Promise<DiagnosticsReport> {
  if (!isRunningInTauri()) {
    return {
      appVersion: "browser-preview",
      capture: {
        permission: "authorized",
        status: "stopped",
        paused: false,
      },
      lastAction: null,
    };
  }
  return invoke<DiagnosticsReport>("diagnostics");
}

/**
 * Pauses capture (fail-open recovery surface). Returns the new paused state.
 *
 * Browser no-op returning `false`, so the web preview stays identical.
 */
export async function pauseCapture(): Promise<boolean> {
  if (!isRunningInTauri()) return false;
  return invoke<boolean>("pause_capture");
}

/**
 * Resumes capture after a pause. Returns the new paused state.
 *
 * Browser no-op returning `false`.
 */
export async function resumeCapture(): Promise<boolean> {
  if (!isRunningInTauri()) return false;
  return invoke<boolean>("resume_capture");
}

/**
 * Subscribes to `ActionReceipt` events emitted by the Rust shell.
 *
 * Returns an unsubscriber. Outside Tauri it is a no-op that is safe to call
 * from React effects.
 */
export function subscribeActionReceipts(
  callback: (receipt: ActionReceipt) => void,
): UnlistenFn {
  if (!isRunningInTauri()) {
    return () => undefined;
  }
  let cancelled = false;
  let unlisten: UnlistenFn | undefined;
  void listen<ActionReceipt>(ACTION_RECEIPT_EVENT, (event) =>
    callback(event.payload),
  ).then((fn) => {
    if (cancelled) {
      fn();
    } else {
      unlisten = fn;
    }
  });
  return () => {
    cancelled = true;
    unlisten?.();
  };
}

/** Rust `DetectionResult` (see `crates/hotwire-adapter-sdk`). */
export interface DetectionResult {
  id: string;
  detected: boolean;
  version?: string | null;
  path?: string | null;
}

/** Rust `ValidationResult` (see `crates/hotwire-adapter-sdk`). */
export interface ValidationResult {
  valid: boolean;
  errors: string[];
}

/**
 * Runs one action through a registered adapter and returns its receipt.
 *
 * Mirrors the `run_adapter_action` command (ADP-001 vertical slice). A `hold`
 * execution that reports `started` stays tracked so the caller can end it with
 * `releaseAdapterAction` or `cancelAdapterAction`. Browser no-op returning
 * `null`.
 */
export async function runAdapterAction(
  adapterId: string,
  actionId: string,
  trigger: "press" | "hold" | "double_press",
  config: Record<string, unknown>,
  physicalCode: string,
): Promise<ActionReceipt | null> {
  if (!isRunningInTauri()) return null;
  return invoke<ActionReceipt>("run_adapter_action", {
    adapterId,
    actionId,
    trigger,
    config,
    physicalCode,
  });
}

/**
 * Ends a tracked hold execution (e.g. releasing a Papegøye push-to-talk key).
 *
 * Mirrors the `release_adapter_action` command. Browser no-op returning `null`.
 */
export async function releaseAdapterAction(
  adapterId: string,
  executionId: string,
  physicalCode: string,
): Promise<ActionReceipt | null> {
  if (!isRunningInTauri()) return null;
  return invoke<ActionReceipt>("release_adapter_action", {
    adapterId,
    executionId,
    physicalCode,
  });
}

/**
 * Cancels a tracked execution. Mirrors the `cancel_adapter_action` command.
 * Browser no-op returning `null`.
 */
export async function cancelAdapterAction(
  adapterId: string,
  executionId: string,
  physicalCode: string,
): Promise<ActionReceipt | null> {
  if (!isRunningInTauri()) return null;
  return invoke<ActionReceipt>("cancel_adapter_action", {
    adapterId,
    executionId,
    physicalCode,
  });
}

/**
 * Probes a registered adapter for machine-level presence.
 *
 * Mirrors the `detect_adapter` command. Browser no-op returning `null`.
 */
export async function detectAdapter(
  adapterId: string,
): Promise<DetectionResult | null> {
  if (!isRunningInTauri()) return null;
  return invoke<DetectionResult>("detect_adapter", { adapterId });
}

/**
 * Validates a binding config against a registered adapter.
 *
 * Mirrors the `validate_adapter_config` command. Browser no-op returning `null`.
 */
export async function validateAdapterConfig(
  adapterId: string,
  config: Record<string, unknown>,
): Promise<ValidationResult | null> {
  if (!isRunningInTauri()) return null;
  return invoke<ValidationResult>("validate_adapter_config", { adapterId, config });
}
