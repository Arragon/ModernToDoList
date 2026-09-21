/**
 * Document session lifecycle for the UI (M3 session commands).
 *
 * Task mutations (`update_task_field`, relations, quick add) all need an open
 * session for the document that owns the task. Sessions are opened lazily on
 * first mutation and reused afterwards; every call degrades gracefully when the
 * backend is unavailable so the UI can disable editing instead of throwing.
 */
import { computed, ref } from "vue";
import type { SessionStatusResponse } from "../ipc/types";
import * as ipc from "../ipc/client";
import { documents, showToast } from "./app-state";

/** document file path -> session id */
const sessionsByPath = ref<Map<string, number>>(new Map());
/** session id -> latest status */
const statusBySession = ref<Map<number, SessionStatusResponse>>(new Map());
/** document id currently driving undo/redo/save availability */
const activeDocumentId = ref<string | null>(null);
/** Paths we already failed to open, so we do not retry on every keystroke. */
const failedPaths = ref<Set<string>>(new Set());

export function filePathForDocumentId(documentId: string): string | null {
  const doc = documents.value.find((d) => d.id === documentId);
  return doc?.file_path ?? null;
}

export function sessionIdForPath(path: string): number | null {
  return sessionsByPath.value.get(path) ?? null;
}

export function setActiveDocument(documentId: string | null): void {
  activeDocumentId.value = documentId;
}

export const activeSessionId = computed<number | null>(() => {
  const documentId = activeDocumentId.value;
  if (!documentId) return null;
  const path = filePathForDocumentId(documentId);
  if (!path) return null;
  return sessionsByPath.value.get(path) ?? null;
});

const activeStatus = computed<SessionStatusResponse | null>(() => {
  const id = activeSessionId.value;
  if (id === null) return null;
  return statusBySession.value.get(id) ?? null;
});

export const canUndo = computed(() => activeStatus.value?.can_undo ?? false);
export const canRedo = computed(() => activeStatus.value?.can_redo ?? false);
export const isDirty = computed(() => activeStatus.value?.is_dirty ?? false);
export const currentRevision = computed(() => activeStatus.value?.current_revision ?? 0);

/** True when we hold a session for the given document (editing is possible). */
export function hasSessionFor(documentId: string): boolean {
  const path = filePathForDocumentId(documentId);
  if (!path) return false;
  return sessionsByPath.value.has(path);
}

/**
 * Opens (or reuses) the editing session for a document.
 * Returns `null` when no session could be established; callers must surface a
 * message rather than assume editing works.
 */
export async function ensureSessionForDocument(documentId: string): Promise<number | null> {
  const path = filePathForDocumentId(documentId);
  if (!path) {
    showToast("Document is not part of the open workspace", "warning");
    return null;
  }
  const existing = sessionsByPath.value.get(path);
  if (typeof existing === "number") {
    activeDocumentId.value = documentId;
    return existing;
  }
  if (failedPaths.value.has(path)) return null;

  try {
    const opened = await ipc.openDocumentSession(path);
    const next = new Map(sessionsByPath.value);
    next.set(path, opened.session_id);
    sessionsByPath.value = next;
    activeDocumentId.value = documentId;
    await refreshSessionStatus(opened.session_id);
    return opened.session_id;
  } catch (err) {
    const failures = new Set(failedPaths.value);
    failures.add(path);
    failedPaths.value = failures;
    showToast(`Could not open an editing session: ${describe(err)}`, "error");
    return null;
  }
}

export async function refreshSessionStatus(sessionId: number): Promise<SessionStatusResponse | null> {
  try {
    const status = await ipc.getSessionStatus(sessionId);
    const next = new Map(statusBySession.value);
    next.set(sessionId, status);
    statusBySession.value = next;
    return status;
  } catch {
    return null;
  }
}

/** Persists the session's document atomically. Returns true on success. */
export async function saveSession(sessionId: number): Promise<boolean> {
  try {
    const result = await ipc.saveDocumentAtomic(sessionId);
    if (!result.success) {
      showToast(`Save failed: ${result.error ?? "unknown error"}`, "error");
      await refreshSessionStatus(sessionId);
      return false;
    }
    await refreshSessionStatus(sessionId);
    showToast("Document saved", "success");
    return true;
  } catch (err) {
    showToast(`Save failed: ${describe(err)}`, "error");
    return false;
  }
}

export async function saveActiveSession(): Promise<boolean> {
  const sessionId = activeSessionId.value;
  if (sessionId === null) {
    showToast("Nothing to save: no document session is open", "warning");
    return false;
  }
  return saveSession(sessionId);
}

export async function undoActiveSession(): Promise<boolean> {
  const sessionId = activeSessionId.value;
  if (sessionId === null) {
    showToast("Nothing to undo: no document session is open", "warning");
    return false;
  }
  try {
    const result = await ipc.undoLastCommand(sessionId);
    await refreshSessionStatus(sessionId);
    if (!result.success) {
      showToast(result.message || "Nothing to undo", "info");
      return false;
    }
    showToast(result.message, "success");
    return true;
  } catch (err) {
    showToast(`Undo failed: ${describe(err)}`, "error");
    return false;
  }
}

export async function redoActiveSession(): Promise<boolean> {
  const sessionId = activeSessionId.value;
  if (sessionId === null) {
    showToast("Nothing to redo: no document session is open", "warning");
    return false;
  }
  try {
    const result = await ipc.redoLastCommand(sessionId);
    await refreshSessionStatus(sessionId);
    if (!result.success) {
      showToast(result.message || "Nothing to redo", "info");
      return false;
    }
    showToast(result.message, "success");
    return true;
  } catch (err) {
    showToast(`Redo failed: ${describe(err)}`, "error");
    return false;
  }
}

/** Marks a session dirty after a successful mutation and refreshes status. */
export async function noteMutation(sessionId: number): Promise<void> {
  await refreshSessionStatus(sessionId);
}

export async function closeAllSessions(): Promise<void> {
  const entries = Array.from(sessionsByPath.value.entries());
  sessionsByPath.value = new Map();
  statusBySession.value = new Map();
  failedPaths.value = new Set();
  activeDocumentId.value = null;
  for (const [, sessionId] of entries) {
    try {
      await ipc.closeDocumentSession(sessionId);
    } catch {
      // The session is gone either way; closing is best-effort.
    }
  }
}

function describe(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  return String(err);
}
