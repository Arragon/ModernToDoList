import { ref } from "vue";
import type { WorkspaceInfo, DocumentInfo, DbStatus } from "../ipc/types";
import * as ipc from "../ipc/client";

export type AppView = "empty" | "workspace" | "loading";
export type ToastType = "info" | "success" | "warning" | "error";

export interface Toast {
  id: number;
  message: string;
  type: ToastType;
  timeout?: number;
}

let toastIdCounter = 0;

export const appView = ref<AppView>("empty");
export const appLoading = ref(false);
export const appLoadingMessage = ref("");

export const workspace = ref<WorkspaceInfo | null>(null);
export const documents = ref<DocumentInfo[]>([]);
export const dbStatus = ref<DbStatus | null>(null);

export const selectedTaskKey = ref<string | null>(null);
export const expandedTaskKeys = ref<Set<string>>(new Set());

export const toasts = ref<Toast[]>([]);

export const confirmDialog = ref({
  visible: false,
  title: "",
  message: "",
  confirmLabel: "Confirm",
  onConfirm: null as (() => void) | null,
});

export function showToast(message: string, type: ToastType = "info", timeout = 3000) {
  const id = ++toastIdCounter;
  toasts.value.push({ id, message, type, timeout });
  if (timeout > 0) {
    setTimeout(() => dismissToast(id), timeout);
  }
}

export function dismissToast(id: number) {
  toasts.value = toasts.value.filter((t) => t.id !== id);
}

export function showConfirm(title: string, message: string, onConfirm: () => void, confirmLabel = "Confirm") {
  confirmDialog.value = {
    visible: true,
    title,
    message,
    confirmLabel,
    onConfirm,
  };
}

export async function openWorkspace(path: string, name: string) {
  appLoading.value = true;
  appLoadingMessage.value = "Opening workspace...";
  try {
    const info = await ipc.createWorkspace(path, name);
    workspace.value = info;
    appView.value = "workspace";
    await refreshDocuments();
    await refreshDbStatus();
    showToast(`Workspace "${info.name}" opened`, "success");
  } catch (e) {
    showToast(`Failed to open workspace: ${e}`, "error");
  } finally {
    appLoading.value = false;
  }
}

export async function loadWorkspace(path: string) {
  appLoading.value = true;
  appLoadingMessage.value = "Loading workspace...";
  try {
    const info = await ipc.openWorkspace(path);
    workspace.value = info;
    appView.value = "workspace";
    await refreshDocuments();
    await refreshDbStatus();
  } catch (e) {
    showToast(`Failed to load workspace: ${e}`, "error");
    appView.value = "empty";
  } finally {
    appLoading.value = false;
  }
}

export async function closeWorkspace() {
  try {
    await ipc.closeWorkspace();
    workspace.value = null;
    documents.value = [];
    selectedTaskKey.value = null;
    appView.value = "empty";
  } catch (e) {
    showToast(`Failed to close workspace: ${e}`, "error");
  }
}

export async function refreshDocuments() {
  try {
    documents.value = await ipc.listDocuments();
  } catch (e) {
    console.error("Failed to list documents:", e);
  }
}

export async function refreshDbStatus() {
  try {
    dbStatus.value = await ipc.getDbStatus();
  } catch (e) {
    console.error("Failed to get DB status:", e);
  }
}

export async function scanAndIndex() {
  appLoading.value = true;
  appLoadingMessage.value = "Scanning and indexing...";
  try {
    const result = await ipc.scanAndIndex();
    await refreshDocuments();
    showToast(`Indexed ${result.total_tasks} tasks from ${result.total_files} files`, "success");
  } catch (e) {
    showToast(`Index failed: ${e}`, "error");
  } finally {
    appLoading.value = false;
  }
}

export async function rebuildIndex() {
  appLoading.value = true;
  appLoadingMessage.value = "Rebuilding index...";
  try {
    const result = await ipc.rebuildIndex();
    await refreshDocuments();
    showToast(`Rebuilt index: ${result.total_tasks} tasks`, "success");
  } catch (e) {
    showToast(`Rebuild failed: ${e}`, "error");
  } finally {
    appLoading.value = false;
  }
}

export function selectTask(key: string | null) {
  selectedTaskKey.value = key;
}

export function toggleExpand(key: string) {
  const s = new Set(expandedTaskKeys.value);
  if (s.has(key)) {
    s.delete(key);
  } else {
    s.add(key);
  }
  expandedTaskKeys.value = s;
}
