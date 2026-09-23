/**
 * Detects whether the Tauri desktop backend is reachable.
 *
 * The same frontend bundle ships inside the Tauri WebView and, for preview
 * purposes, in a plain browser. In a browser there is no `__TAURI_INTERNALS__`,
 * so every `invoke` rejects with a low-level TypeError. Rather than leaking
 * that to the user we expose a single reactive flag plus a friendly error
 * string, letting the UI degrade to a clearly-labelled read-only preview.
 */
import { ref } from "vue";
import { t } from "./i18n";

interface TauriWindow {
  __TAURI_INTERNALS__?: unknown;
}

export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && !!(window as TauriWindow).__TAURI_INTERNALS__;
}

/** True only when a desktop backend can actually service IPC calls. */
export const backendAvailable = ref(isTauriRuntime());

/**
 * Maps a failed IPC call to a user-facing message. When the backend is simply
 * absent (web preview) we say so plainly instead of dumping the raw TypeError.
 */
export function describeBackendError(err: unknown): string {
  if (!backendAvailable.value) return t("web.error.backend");
  if (err instanceof Error) return err.message;
  if (typeof err === "string") return err;
  return String(err);
}
