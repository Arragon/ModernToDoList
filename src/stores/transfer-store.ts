/**
 * Cross-document transfer UX state (RD-M8 / INH-1083).
 *
 * The backend transfer engine commits target-first and keeps a crash journal,
 * so this store mirrors that failure model exactly: a move only drops its
 * source row after the backend confirms the outcome, error strings are
 * classified by their documented prefixes into the five result states, and
 * interrupted transactions surface through the recovery panel until the user
 * either completes the move or keeps the duplicate.
 */
import { computed, ref } from "vue";
import type { UnlistenFn } from "@tauri-apps/api/event";
import type {
  TransferOperationKind, TransferOutcomeDto, TransferPreviewDto,
  TransferProgressEvent, TransferRecoveryReportDto, TransferTargetInfo, TransferWarning,
} from "../ipc/types";
import * as ipc from "../ipc/client";
import type { IpcResult } from "../ipc/safe";
import { ipcFailureMessage } from "../ipc/safe";
import { Commands } from "../ipc/commands";
import { isCommandAvailable, markCommandUnavailable } from "./capability-store";
import { filePathForDocumentId, forgetSessionForPath } from "./session-store";
import { showConfirm, showToast, selectedTaskKey, selectTask } from "./app-state";
import { allTasks, loadTasks } from "./task-store";
import { loadDependencies } from "./relation-store";

// ── Picker state ─────────────────────────────────────────────────────────────

/** Drag payload MIME for cross-document task transfer (TaskRow → Sidebar). */
export const TASK_DRAG_MIME = "application/x-mtdl-task";

export interface TransferPickerState {
  taskKey: string;
  taskTitle: string;
  sourceDocumentId: string;
  operation: TransferOperationKind;
}

const pickerState = ref<TransferPickerState | null>(null);
const targets = ref<TransferTargetInfo[]>([]);
const targetsLoading = ref(false);

export const transferAvailable = computed(() =>
  isCommandAvailable(Commands.COPY_TASK_BETWEEN_DOCUMENTS),
);
export const activePicker = computed(() => pickerState.value);
export const transferTargets = computed(() => targets.value);
export const transferTargetsLoading = computed(() => targetsLoading.value);

export async function openTransferPicker(state: TransferPickerState): Promise<void> {
  pickerState.value = state;
  targetsLoading.value = true;
  const result = await ipc.listTransferTargets(state.sourceDocumentId);
  targetsLoading.value = false;
  if (!result.ok) {
    if (result.missing) markCommandUnavailable(Commands.LIST_TRANSFER_TARGETS);
    showToast(ipcFailureMessage(result, "List transfer targets"), result.missing ? "warning" : "error");
    targets.value = [];
    return;
  }
  targets.value = result.value;
}

export function closeTransferPicker(): void {
  pickerState.value = null;
  targets.value = [];
}

/** Side-effect-free plan preview for the picker's confirm step. */
export async function previewTransferTo(
  targetDocumentId: string,
): Promise<IpcResult<TransferPreviewDto> | null> {
  const picker = pickerState.value;
  if (!picker) return null;
  return ipc.previewTransfer(
    picker.sourceDocumentId, targetDocumentId, picker.taskKey, picker.operation,
  );
}

/** Human-readable rendering of a non-fatal transfer warning. */
export function describeTransferWarning(w: TransferWarning): string {
  switch (w.kind) {
    case "external_dependency_preserved":
      return `Dependency ${w.task_id} → ${w.dep_task_id} stays linked to ${w.source_doc}`;
    case "asset_missing":
      return `Attachment not found on disk: ${w.path}`;
    case "asset_absolute_reference_kept":
      return `Absolute attachment path kept as-is: ${w.path}`;
    case "asset_url_kept":
      return `URL attachment kept as-is: ${w.url}`;
    case "asset_collision_renamed":
      return `Attachment renamed to avoid a collision: ${w.from} → ${w.to}`;
    case "non_numeric_task_id":
      return `Non-numeric task id preserved: ${w.id}`;
    case "orphaned_source_dependency":
      return `Dependency ${w.task_id} → ${w.dep_task_id} pointed outside the moved subtree and was dropped`;
  }
}

/**
 * Drag-and-drop entry point (Sidebar document drop target): skips the picker
 * but keeps the preview + warning confirmation pipeline. The caller is
 * responsible for the mandatory explicit confirmation before calling this.
 */
export async function startDirectTransfer(
  state: TransferPickerState,
  targetDocumentId: string,
): Promise<void> {
  pickerState.value = state;
  await confirmTransfer(targetDocumentId);
}

/**
 * Previews, then (with explicit confirmation when the plan carries warnings)
 * executes the pending picker transfer.
 */
export async function confirmTransfer(targetDocumentId: string): Promise<void> {
  const picker = pickerState.value;
  if (!picker) return;
  const preview = await previewTransferTo(targetDocumentId);
  if (!preview) return;
  if (!preview.ok) {
    if (preview.missing) markCommandUnavailable(Commands.PREVIEW_TRANSFER);
    showToast(ipcFailureMessage(preview, "Transfer preview"), preview.missing ? "warning" : "error");
    return;
  }
  const warnings = preview.value.warnings;
  if (warnings.length > 0) {
    const lines = warnings.map((w) => `• ${describeTransferWarning(w)}`).join("\n");
    const verb = picker.operation === "move" ? "Move" : "Copy";
    showConfirm(
      `${verb} with ${warnings.length} warning(s)?`,
      `${preview.value.task_count} task(s) will be transferred.\n${lines}`,
      () => { void executeTransfer(targetDocumentId); },
      verb,
    );
    return;
  }
  await executeTransfer(targetDocumentId);
}

// ── Execution + progress ─────────────────────────────────────────────────────

export interface ActiveTransfer {
  operation: TransferOperationKind;
  phase: TransferProgressEvent["phase"];
  detail: string;
}

export type TransferResultKind =
  | "completed"
  | "recovery_required"
  | "failed_before_target_commit"
  | "failed";

export interface TransferResult {
  kind: TransferResultKind;
  operation: TransferOperationKind;
  message: string;
  transactionId: string | null;
  outcome: TransferOutcomeDto | null;
}

const active = ref<ActiveTransfer | null>(null);
const result = ref<TransferResult | null>(null);
const busy = ref(false);

export const activeTransfer = computed(() => active.value);
export const transferResult = computed(() => result.value);
export const transferBusy = computed(() => busy.value);

export function dismissTransferResult(): void {
  result.value = null;
}

async function executeTransfer(targetDocumentId: string): Promise<void> {
  const picker = pickerState.value;
  if (!picker || busy.value) return;
  busy.value = true;
  active.value = { operation: picker.operation, phase: "planning", detail: "" };
  try {
    const invoke = picker.operation === "move"
      ? ipc.moveTaskBetweenDocuments
      : ipc.copyTaskBetweenDocuments;
    const outcome = await invoke(picker.sourceDocumentId, targetDocumentId, picker.taskKey);
    if (!outcome.ok) {
      if (outcome.missing) {
        markCommandUnavailable(
          picker.operation === "move"
            ? Commands.MOVE_TASK_BETWEEN_DOCUMENTS
            : Commands.COPY_TASK_BETWEEN_DOCUMENTS,
        );
        result.value = {
          kind: "failed", operation: picker.operation, transactionId: null, outcome: null,
          message: "Transfer is not available yet (backend command missing)",
        };
        return;
      }
      result.value = classifyTransferError(picker.operation, outcome.error);
      return;
    }
    result.value = {
      kind: "completed", operation: picker.operation,
      message: "", transactionId: outcome.value.transaction_id, outcome: outcome.value,
    };
    pickerState.value = null;
    targets.value = [];
    await afterTransferCommitted([picker.sourceDocumentId, targetDocumentId]);
  } finally {
    busy.value = false;
    active.value = null;
  }
}

/**
 * The backend reindexes both documents and drops their editing sessions after
 * a transfer; the frontend mirrors that by forgetting the same sessions and
 * reloading index-derived state. For a move this is also the moment the source
 * row disappears — never earlier.
 */
async function afterTransferCommitted(documentIds: string[]): Promise<void> {
  for (const id of documentIds) {
    const path = filePathForDocumentId(id);
    if (path) forgetSessionForPath(path);
  }
  await loadTasks();
  await loadDependencies(true);
  // A move deletes the source row: drop a now-dangling selection.
  const selected = selectedTaskKey.value;
  if (selected && !allTasks.value.some((t) => t.task_key === selected)) {
    selectTask(null);
  }
}

// ── Error classification (prefix contract with commands/transfer.rs) ─────────

function classifyTransferError(operation: TransferOperationKind, error: string): TransferResult {
  const base = { operation, outcome: null };
  if (error.startsWith("failed_before_target_commit:")) {
    return {
      ...base, kind: "failed_before_target_commit", transactionId: null,
      message: error.slice("failed_before_target_commit:".length).trim(),
    };
  }
  if (error.startsWith("recovery_required:")) {
    // Wire format: `recovery_required:{txn}:{msg}`; txn is empty when the
    // failing variant carries no transaction id.
    const rest = error.slice("recovery_required:".length);
    const sep = rest.indexOf(":");
    const transactionId = sep >= 0 ? rest.slice(0, sep) || null : null;
    return {
      ...base, kind: "recovery_required", transactionId,
      message: (sep >= 0 ? rest.slice(sep + 1) : rest).trim(),
    };
  }
  if (error.startsWith("session_dirty:")) {
    return {
      ...base, kind: "failed", transactionId: null,
      message: `Save or undo the open changes first: ${error.slice("session_dirty:".length).trim()}`,
    };
  }
  if (error.startsWith("failed:")) {
    return {
      ...base, kind: "failed", transactionId: null,
      message: error.slice("failed:".length).trim(),
    };
  }
  return { ...base, kind: "failed", transactionId: null, message: error };
}

// ── Undo ─────────────────────────────────────────────────────────────────────

export async function undoTransferById(transactionId: string): Promise<boolean> {
  const ack = await ipc.undoTransfer(transactionId);
  if (!ack.ok) {
    if (ack.missing) markCommandUnavailable(Commands.UNDO_TRANSFER);
    showToast(ipcFailureMessage(ack, "Undo transfer"), ack.missing ? "warning" : "error");
    return false;
  }
  if (!ack.value.success) {
    showToast(ack.value.message || "Transfer could not be undone", "warning");
    return false;
  }
  showToast(ack.value.message || "Transfer undone", "success");
  dismissTransferResult();
  // Undo rewrites both documents again; sessions were dropped backend-side.
  await loadTasks();
  await loadDependencies(true);
  return true;
}

// ── Crash recovery ───────────────────────────────────────────────────────────

const reports = ref<TransferRecoveryReportDto[]>([]);
const recoveryScanned = ref(false);

export const recoveryReports = computed(() => reports.value);
export const hasRecoveryReports = computed(() => reports.value.length > 0);

/** Scans the transfer journal; intended to run once after a workspace opens. */
export async function runRecoveryScan(force = false): Promise<void> {
  if (recoveryScanned.value && !force) return;
  recoveryScanned.value = true;
  const result = await ipc.recoverTransfers();
  if (!result.ok) {
    if (result.missing) markCommandUnavailable(Commands.RECOVER_TRANSFERS);
    return;
  }
  reports.value = result.value;
  if (result.value.length > 0) {
    showToast(
      `${result.value.length} interrupted transfer(s) recovered — review them in the transfer panel`,
      "warning",
      6000,
    );
  }
}

/**
 * Resolves a `completed_with_duplicate` report by deleting the retained source
 * subtree (explicit user confirmation happens in the panel component).
 */
export async function completeRecovery(transactionId: string): Promise<boolean> {
  const result = await ipc.completeInterruptedMove(transactionId);
  if (!result.ok) {
    if (result.missing) markCommandUnavailable(Commands.COMPLETE_INTERRUPTED_MOVE);
    showToast(ipcFailureMessage(result, "Complete interrupted move"), result.missing ? "warning" : "error");
    return false;
  }
  reports.value = reports.value.filter((r) => r.transaction_id !== transactionId);
  showToast("Interrupted move completed: the source copy was removed", "success");
  await loadTasks();
  await loadDependencies(true);
  return true;
}

/** Keeps the duplicate: drops the report without touching either document. */
export function keepRecoveryDuplicate(transactionId: string): void {
  reports.value = reports.value.filter((r) => r.transaction_id !== transactionId);
}

export function resetTransferState(): void {
  pickerState.value = null;
  targets.value = [];
  active.value = null;
  result.value = null;
  reports.value = [];
  recoveryScanned.value = false;
}

// ── Progress event listener ──────────────────────────────────────────────────

let unlistenProgress: UnlistenFn | null = null;
let listenerStarted = false;

/**
 * Subscribes to `transfer://progress`. No-op outside a Tauri webview (plain
 * browser dev), mirroring the drag-drop wiring in AttachmentListEditor.
 */
export async function initTransferProgressListener(): Promise<void> {
  if (listenerStarted) return;
  listenerStarted = true;
  try {
    const { listen } = await import("@tauri-apps/api/event");
    unlistenProgress = await listen<TransferProgressEvent>("transfer://progress", (event) => {
      const payload = event.payload;
      const detail = payload.phase === "staging_assets"
        ? `${payload.done}/${payload.total}`
        : "";
      if (active.value) {
        active.value = { ...active.value, phase: payload.phase, detail };
      }
    });
  } catch {
    listenerStarted = false;
  }
}

export function disposeTransferProgressListener(): void {
  if (unlistenProgress) {
    unlistenProgress();
    unlistenProgress = null;
  }
  listenerStarted = false;
}
