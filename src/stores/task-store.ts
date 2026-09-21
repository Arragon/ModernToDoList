import { ref, computed } from "vue";
import type { TaskSummary } from "../ipc/types";
import * as ipc from "../ipc/client";
import { selectedTaskKey, expandedTaskKeys, showToast } from "./app-state";
import { isFilterActive, applyFilters } from "./filter-state";

// All tasks loaded from the index
export const allTasks = ref<TaskSummary[]>([]);
export const tasksLoading = ref(false);

// Build a map for fast lookup
const taskMap = computed(() => {
  const map = new Map<string, TaskSummary>();
  for (const t of allTasks.value) {
    map.set(t.task_key, t);
  }
  return map;
});

// Children lookup
const childrenMap = computed(() => {
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

// Flattened visible rows (respecting expand/collapse)
export interface TaskRow {
  task: TaskSummary;
  depth: number;
  hasChildren: boolean;
  isExpanded: boolean;
}

export const visibleRows = computed<TaskRow[]>(() => {
  const rows: TaskRow[] = [];
  const expanded = expandedTaskKeys.value;
  const filterVisible = isFilterActive.value ? applyFilters(allTasks.value) : null;

  function walk(parentKey: string, depth: number) {
    const children = childrenMap.value.get(parentKey) ?? [];
    for (const child of children) {
      const key = child.task_key;
      // If filter is active, skip tasks not in the visible set
      if (filterVisible && !filterVisible.has(key)) continue;
      const kids = childrenMap.value.get(key) ?? [];
      const hasChildren = kids.length > 0;
      // When filter is active, auto-expand all so matched items are visible
      const isExpanded = filterVisible ? hasChildren : expanded.has(key);
      rows.push({ task: child, depth, hasChildren, isExpanded });
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

// Load all tasks from the index
export async function loadTasks(documentId?: string) {
  tasksLoading.value = true;
  try {
    const result = await ipc.queryTasks(documentId);
    allTasks.value = result.tasks;
  } catch (e) {
    showToast(`Failed to load tasks: ${e}`, "error");
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
  const rows = visibleRows.value;
  if (rows.length === 0) return null;
  const currentKey = selectedTaskKey.value;
  const currentIdx = rows.findIndex((r) => r.task.task_key === currentKey);
  const nextIdx = currentIdx < 0 ? 0 : Math.max(0, Math.min(rows.length - 1, currentIdx + delta));
  return rows[nextIdx].task.task_key;
}
