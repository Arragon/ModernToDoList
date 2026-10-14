/**
 * Fail-soft wrapper around Tauri's `invoke`.
 *
 * Backend commands for M6/M9/M10 are being written concurrently with this
 * frontend. A command that is not registered yet must never produce an
 * unhandled rejection: `safeInvoke` normalises every failure into an
 * `IpcResult` and records missing commands in the capability store so bound
 * controls render disabled.
 */
import { invoke } from "@tauri-apps/api/core";
import { markCommandFailing, markCommandUnavailable } from "../stores/capability-store";

export type IpcResult<T> =
  | { ok: true; value: T }
  | { ok: false; error: string; missing: boolean };

/** Tauri rejects with a string for unregistered commands. */
export function isMissingCommandError(err: unknown): boolean {
  const text = describeIpcError(err);
  return /not found|not registered|not allowed|unknown command/i.test(text);
}

export function describeIpcError(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  if (err && typeof err === "object") {
    const maybeMessage = (err as { message?: unknown }).message;
    if (typeof maybeMessage === "string") return maybeMessage;
  }
  return String(err);
}

export async function safeInvoke<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<IpcResult<T>> {
  try {
    const value = await invoke<T>(command, args);
    return { ok: true, value };
  } catch (err) {
    const missing = isMissingCommandError(err);
    if (missing) {
      markCommandUnavailable(command);
    } else {
      markCommandFailing(command);
    }
    return { ok: false, error: describeIpcError(err), missing };
  }
}

/** `safeInvoke` for commands that return nothing on success. */
export async function safeInvokeVoid(
  command: string,
  args?: Record<string, unknown>,
): Promise<IpcResult<null>> {
  const result = await safeInvoke<null>(command, args);
  if (result.ok) return { ok: true, value: null };
  return result;
}

/** Human-readable label for a failed result, used in toasts. */
export function ipcFailureMessage<T>(result: IpcResult<T>, context: string): string {
  if (result.ok) return "";
  return result.missing
    ? `${context} is not available yet (backend command missing)`
    : `${context}: ${result.error}`;
}
