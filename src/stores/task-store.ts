import { computed, ref } from "vue";
import type { TaskSummary } from "../ipc/types";
import * as ipc from "../ipc/client";
import { ipcFailureMessage } from "../ipc/safe";
import { describeBackendError } from "../app/backend";
import { selectedTaskKey, expandedTaskKeys, documents, showToast, workspace, scanAndIndex, refreshDocuments } from "./app-state";
import { isFilterActive, applyFilters, filterState } from "./filter-state";
import {
  participantsByTaskMap, isBlockedBy, dependencyMap,
} from "./relation-store";
import { ensureSessionForDocument, noteMutation } from "./session-store";

// All tasks loaded from the index
export const allTasks = ref<TaskSummary[]>([]);
export const tasksLoading = ref(false);

/** task_key -> tags, cached as the inspector loads them (drives the tag filter). */
const tagsByTask = ref<Map<string, string[]>>(new Map());

/** Keys that the tree should scroll to once rows have been recomputed. */
export const pendingScrollKey = ref<string | null>(null);

// Build a map for fast lookup
export const taskMap = computed(() => {
  const map = new Map<string, TaskSummary>();
  for (const t of allTasks.value) {
    map.set(t.task_key, t);
  }
  return map;
});

// Children lookup
export const childrenMap = computed(() => {
  const map = new Map<string, TaskSummary[]>();
  for (const t of allTasks.value) {
    const parent = t.parent_key ?? "__root__";
    const list = map.get(parent) ?? [];
    list.push(t);
    map.set(parent, list);
  }
  // Sort each children list by position
  for (const list of map.values()) {
    list.sort((a, b) => a.position - b.position);
  }
  return map;
});

/** Correct child count for the TaskRow badge (Phase 0.1 fix). */
export function childCount(taskKey: string): number {
  return childrenMap.value.get(taskKey)?.length ?? 0;
}

// Flattened visible rows (respecting expand/collapse and grouping)
export interface TaskRow {
  task: TaskSummary;
  depth: number;
  hasChildren: boolean;
  isExpanded: boolean;
  /** "group" rows are synthetic headers produced by a grouping view mode. */
  kind: "task" | "group";
  groupLabel: string;
  groupSize: number;
}

/** Synthetic group header row (participant grouping, RD-M6-008). */
function groupRow(label: string, size: number, collapsed: boolean): TaskRow {
  const task: TaskSummary = {
    task_key: `__group__:${label}`,
    document_id: "",
    title: label,
    priority: 0,
    status: "",
    percent_done: 0,
    risk: 0,
    start_date: null,
    due_date: null,
    completed_date: null,
    parent_key: null,
    position: 0,
  };
  return {
    task,
    depth: 0,
    hasChildren: size > 0,
    isExpanded: !collapsed,
    kind: "group",
    groupLabel: label,
    groupSize: size,
  };
}

function taskRow(task: TaskSummary, depth: number, expanded: boolean): TaskRow {
  return {
    task,
    depth,
    hasChildren: childCount(task.task_key) > 0,
    isExpanded: expanded,
    kind: "task",
    groupLabel: "",
    groupSize: 0,
  };
}

/** Collapsed group headers in a grouping view mode. */
export const collapsedGroupKeys = ref<Set<string>>(new Set());

export function toggleGroup(label: string): void {
  const next = new Set(collapsedGroupKeys.value);
  if (next.has(label)) next.delete(label);
  else next.add(label);
  collapsedGroupKeys.value = next;
}

const UNASSIGNED = "Unassigned";

export const visibleRows = computed<TaskRow[]>(() => {
  const rows: TaskRow[] = [];
  const expanded = expandedTaskKeys.value;
  const context = { participantsByTask: participantsByTaskMap.value, tagsByTask: tagsByTask.value };
  const filterVisible = isFilterActive.value ? applyFilters(allTasks.value, context) : null;

  function matches(key: string): boolean {
    return !filterVisible || filterVisible.has(key);
  }

  if (filterState.value.groupBy === "participant") {
    // Participant grouping view mode: one group per participant name.
    const groups = new Map<string, TaskSummary[]>();
    for (const t of allTasks.value) {
      if (!matches(t.task_key)) continue;
      const names = participantsByTaskMap.value.get(t.task_key);
      const buckets = names && names.length > 0 ? names : [UNASSIGNED];
      for (const name of buckets) {
        const list = groups.get(name) ?? [];
        list.push(t);
        groups.set(name, list);
      }
    }

    const ordered = Array.from(groups.keys()).sort((a, b) => {
      if (a === UNASSIGNED) return 1;
      if (b === UNASSIGNED) return -1;
      return a.localeCompare(b);
    });

    for (const label of ordered) {
      const members = (groups.get(label) ?? []).slice()
        .sort((a, b) => a.position - b.position || a.title.localeCompare(b.title));
      const collapsed = collapsedGroupKeys.value.has(label);
      rows.push(groupRow(label, members.length, collapsed));
      if (collapsed) continue;
      for (const member of members) {
        rows.push(taskRow(member, 1, expanded.has(member.task_key)));
      }
    }
    return rows;
  }

  function walk(parentKey: string, depth: number) {
    const children = childrenMap.value.get(parentKey) ?? [];
    for (const child of children) {
      const key = child.task_key;
      // If filter is active, skip tasks not in the visible set
      if (!matches(key)) continue;
      const hasChildren = childCount(key) > 0;
      // When filter is active, auto-expand all so matched items are visible
      const isExpanded = filterVisible ? hasChildren : expanded.has(key);
      rows.push(taskRow(child, depth, isExpanded));
      if (isExpanded && hasChildren) {
        walk(key, depth + 1);
      }
    }
  }

  walk("__root__", 0);
  return rows;
});

// Selected task
export const selectedTask = computed(() => {
  const key = selectedTaskKey.value;
  if (!key) return null;
  return taskMap.value.get(key) ?? null;
});

// ── Blocking state (M6 dependencies) ─────────────────────────────────────────

const FINISHED_STATUSES = new Set(["Completed", "Cancelled"]);

function isFinished(taskKey: string): boolean {
  const task = taskMap.value.get(taskKey);
  return task ? FINISHED_STATUSES.has(task.status) : false;
}

function existsInIndex(taskKey: string): boolean {
  return taskMap.value.has(taskKey);
}

/** task_key -> true when at least one dependency is unfinished/unresolved/circular. */
export const blockedKeys = computed<ReadonlySet<string>>(() => {
  const blocked = new Set<string>();
  for (const key of dependencyMap.value.keys()) {
    if (isBlockedBy(key, isFinished, existsInIndex)) blocked.add(key);
  }
  return blocked;
});

export function isTaskBlocked(taskKey: string): boolean {
  return blockedKeys.value.has(taskKey);
}

// ── Loading ──────────────────────────────────────────────────────────────────

export async function loadTasks(documentId?: string) {
  tasksLoading.value = true;
  try {
    const result = await ipc.queryTasks(documentId);
    allTasks.value = result.tasks;
  } catch (e) {
    showToast(describeBackendError(e), "error");
  } finally {
    tasksLoading.value = false;
  }
}

// Expand all root tasks by default
export function expandRootTasks() {
  const keys = new Set<string>();
  for (const t of allTasks.value) {
    if (!t.parent_key) {
      keys.add(t.task_key);
    }
  }
  expandedTaskKeys.value = keys;
}

// Get task by key
export function getTask(key: string): TaskSummary | undefined {
  return taskMap.value.get(key);
}

// Get children of a task
export function getChildren(key: string): TaskSummary[] {
  return childrenMap.value.get(key) ?? [];
}

// Navigate to next/previous visible row
export function navigateVisibleRow(delta: number): string | null {
  const rows = visibleRows.value.filter((r) => r.kind === "task");
  if (rows.length === 0) return null;
  const currentKey = selectedTaskKey.value;
  const currentIdx = rows.findIndex((r) => r.task.task_key === currentKey);
  const nextIdx = currentIdx < 0 ? 0 : Math.max(0, Math.min(rows.length - 1, currentIdx + delta));
  return rows[nextIdx].task.task_key;
}

/**
 * Selects a task, expands every ancestor and asks the tree to scroll to it.
 * Used by global search, the command palette and saved views (task jump).
 */
export function jumpToTask(taskKey: string): boolean {
  const task = taskMap.value.get(taskKey);
  if (!task) {
    showToast("Task is not in the current index", "warning");
    return false;
  }
  const expanded = new Set(expandedTaskKeys.value);
  let cursor: TaskSummary | undefined = task;
  while (cursor?.parent_key) {
    expanded.add(cursor.parent_key);
    cursor = taskMap.value.get(cursor.parent_key);
  }
  expandedTaskKeys.value = expanded;
  collapsedGroupKeys.value = new Set();
  selectedTaskKey.value = taskKey;
  pendingScrollKey.value = taskKey;
  return true;
}

/** Index of a task key within the currently visible rows (-1 when hidden). */
export function visibleRowIndex(taskKey: string): number {
  return visibleRows.value.findIndex((r) => r.task.task_key === taskKey);
}

export function clearPendingScroll(): void {
  pendingScrollKey.value = null;
}

// ── Tag cache ────────────────────────────────────────────────────────────────

export function cacheTags(taskKey: string, tags: string[]): void {
  const next = new Map(tagsByTask.value);
  next.set(taskKey, tags);
  tagsByTask.value = next;
}

export function tagsFor(taskKey: string): string[] {
  return tagsByTask.value.get(taskKey) ?? [];
}

// ── Mutations ────────────────────────────────────────────────────────────────

export type EditableTaskField =
  | "title"
  | "status"
  | "priority"
  | "percent_done"
  | "risk"
  | "start_date"
  | "due_date"
  | "completed_date";

function patchLocalTask(taskKey: string, field: EditableTaskField, value: string): void {
  const index = allTasks.value.findIndex((t) => t.task_key === taskKey);
  if (index < 0) return;
  const next: TaskSummary = { ...allTasks.value[index] };

  switch (field) {
    case "title":
      next.title = value;
      break;
    case "status":
      next.status = value;
      if (value === "Completed" && !next.completed_date) {
        next.completed_date = new Date().toISOString().slice(0, 10);
      }
      break;
    case "start_date":
      next.start_date = value.length > 0 ? value : null;
      break;
    case "due_date":
      next.due_date = value.length > 0 ? value : null;
      break;
    case "completed_date":
      next.completed_date = value.length > 0 ? value : null;
      break;
    case "priority":
      next.priority = toInt(value);
      break;
    case "percent_done":
      next.percent_done = toInt(value);
      break;
    case "risk":
      next.risk = toInt(value);
      break;
  }

  const updated = allTasks.value.slice();
  updated[index] = next;
  allTasks.value = updated;
}

function toInt(value: string): number {
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) ? parsed : 0;
}

/**
 * Single entry point for inspector / TaskRow field edits.
 * Opens the document session lazily, calls `update_task_field`, patches the
 * local index on success and reports failures through a toast.
 */
export async function updateTaskField(
  taskKey: string,
  field: EditableTaskField,
  value: string,
): Promise<boolean> {
  const task = taskMap.value.get(taskKey);
  if (!task) {
    showToast("Task is not in the current index", "warning");
    return false;
  }
  const sessionId = await ensureSessionForDocument(task.document_id);
  if (sessionId === null) return false;

  const result = await ipc.updateTaskField(sessionId, taskKey, field, value);
  if (!result.ok) {
    showToast(ipcFailureMessage(result, `Update ${field}`), result.missing ? "warning" : "error");
    return false;
  }
  if (!result.value.success) {
    showToast(`Update ${field} was rejected by the backend`, "error");
    return false;
  }
  patchLocalTask(taskKey, field, value);
  await noteMutation(sessionId);
  return true;
}

/** Checkbox toggle: Completed <-> the task's previous actionable status. */
export async function toggleTaskStatus(taskKey: string): Promise<boolean> {
  const task = taskMap.value.get(taskKey);
  if (!task) return false;
  const next = task.status === "Completed" ? "Not Started" : "Completed";
  return updateTaskField(taskKey, "status", next);
}

export async function setTaskTags(taskKey: string, tags: string[]): Promise<boolean> {
  const task = taskMap.value.get(taskKey);
  if (!task) return false;
  const sessionId = await ensureSessionForDocument(task.document_id);
  if (sessionId === null) return false;

  const result = await ipc.setTaskTags(sessionId, taskKey, task.document_id, tags);
  if (!result.ok) {
    showToast(ipcFailureMessage(result, "Update tags"), result.missing ? "warning" : "error");
    return false;
  }
  cacheTags(taskKey, tags);
  await noteMutation(sessionId);
  return true;
}

export async function deleteTask(taskKey: string): Promise<boolean> {
  const task = taskMap.value.get(taskKey);
  if (!task) return false;
  const sessionId = await ensureSessionForDocument(task.document_id);
  if (sessionId === null) return false;

  const result = await ipc.deleteTask(sessionId, taskKey);
  if (!result.ok) {
    showToast(ipcFailureMessage(result, "Delete task"), result.missing ? "warning" : "error");
    return false;
  }
  allTasks.value = allTasks.value.filter((t) => t.task_key !== taskKey);
  if (selectedTaskKey.value === taskKey) selectedTaskKey.value = null;
  await noteMutation(sessionId);
  return true;
}

/**
 * Creates a task through the session (used by Quick Add and `task.addRoot`).
 * Falls back to `add_task` only; when that command is missing the caller shows
 * the reason and nothing is mutated locally.
 */
export async function createTask(input: {
  title: string;
  documentId: string | null;
  parentKey: string | null;
  priority: number;
  tags: string[];
  participants: string[];
  dueDate: string | null;
  startDate: string | null;
  /** Pre-allocated TaskId (from `allocate_task_id`); null lets the backend pick. */
  taskKey?: string | null;
}): Promise<string | null> {
  const documentId = input.documentId ?? (firstDocumentId() ?? await bootstrapDefaultDocument());
  if (!documentId) {
    showToast("No document is available to add a task to", "warning");
    return null;
  }
  const title = input.title.trim();
  if (!title) {
    showToast("A task needs a title", "warning");
    return null;
  }
  const sessionId = await ensureSessionForDocument(documentId);
  if (sessionId === null) return null;

  const result = await ipc.addTask({
    session_id: sessionId,
    document_id: documentId,
    task_key: input.taskKey ?? null,
    title,
    parent_key: input.parentKey,
    priority: input.priority,
    status: "Not Started",
    due_date: input.dueDate,
    start_date: input.startDate,
    tags: input.tags,
    participants: input.participants,
  });
  if (!result.ok) {
    showToast(ipcFailureMessage(result, "Add task"), result.missing ? "warning" : "error");
    return null;
  }
  if (!result.value.success) {
    showToast("Backend rejected the new task", "error");
    return null;
  }

  const created: TaskSummary = {
    task_key: result.value.task_key,
    document_id: documentId,
    title,
    priority: input.priority,
    status: "Not Started",
    percent_done: 0,
    risk: 0,
    start_date: input.startDate,
    due_date: input.dueDate,
    completed_date: null,
    parent_key: input.parentKey,
    position: getChildren(input.parentKey ?? "__root__").length,
  };
  allTasks.value = [...allTasks.value, created];
  if (input.tags.length > 0) cacheTags(created.task_key, input.tags);
  if (input.parentKey) {
    const expanded = new Set(expandedTaskKeys.value);
    expanded.add(input.parentKey);
    expandedTaskKeys.value = expanded;
  }
  await noteMutation(sessionId);
  return created.task_key;
}

function firstDocumentId(): string | null {
  return documents.value.length > 0 ? documents.value[0].id : null;
}

/** Minimal, valid empty TDL document used to bootstrap a brand-new workspace. */
const EMPTY_TDL = `<?xml version="1.0" encoding="utf-8"?>
<TODOLIST PROJECTNAME="" EARLIESTDUEDATE="0.00000000" LASTMOD="0.00000000" LASTMODSTRING="" FILENAME="ToDoList.tdl" NEXTUNIQUEID="1" FILEVERSION="43" APPVER="9.0.14.0" FILEFORMAT="12">
</TODOLIST>
`;

/**
 * Creates the first task document in a workspace that has none, so a brand-new
 * workspace can accept its first task. Writes a minimal TDL via the existing
 * serialize command, then re-scans so the index (and `documents`) picks it up.
 * Returns the new document id, or null when it could not be created.
 */
export async function bootstrapDefaultDocument(): Promise<string | null> {
  const root = workspace.value?.root_path;
  if (!root) return null;
  const path = `${root.replace(/[\\/]$/, "")}\\ToDoList.tdl`;
  try {
    await ipc.serializeAndWriteDocument(EMPTY_TDL, path, "utf-8");
  } catch (e) {
    showToast(describeBackendError(e), "error");
    return null;
  }
  await scanAndIndex();
  await refreshDocuments();
  return firstDocumentId();
}
