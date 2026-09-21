import { ref, computed } from "vue";
import type { TaskSummary } from "../ipc/types";

export interface FilterState {
  titleKeyword: string;
  tags: string[];
  categories: string[];
  dueDateRange: "all" | "overdue" | "today" | "this-week" | "no-date";
  statusFilter: string | null;
}

export const filterState = ref<FilterState>({
  titleKeyword: "",
  tags: [],
  categories: [],
  dueDateRange: "all",
  statusFilter: null,
});

export const isFilterActive = computed(() => {
  const f = filterState.value;
  return (
    f.titleKeyword.length > 0 ||
    f.tags.length > 0 ||
    f.categories.length > 0 ||
    f.dueDateRange !== "all" ||
    f.statusFilter !== null
  );
});

export function clearFilters() {
  filterState.value = {
    titleKeyword: "",
    tags: [],
    categories: [],
    dueDateRange: "all",
    statusFilter: null,
  };
}

// Apply filters to a task list, returning matching task keys
export function applyFilters(tasks: TaskSummary[]): Set<string> {
  const f = filterState.value;
  if (!isFilterActive.value) {
    return new Set(tasks.map((t) => t.task_key));
  }

  const matched = new Set<string>();
  const now = new Date();
  now.setHours(0, 0, 0, 0);

  for (const t of tasks) {
    let pass = true;

    // Title keyword
    if (f.titleKeyword) {
      const kw = f.titleKeyword.toLowerCase();
      if (!t.title.toLowerCase().includes(kw)) {
        pass = false;
      }
    }

    // Status filter
    if (pass && f.statusFilter && t.status !== f.statusFilter) {
      pass = false;
    }

    // Due date range
    if (pass && f.dueDateRange !== "all") {
      if (f.dueDateRange === "no-date") {
        if (t.due_date) pass = false;
      } else if (!t.due_date) {
        pass = false;
      } else {
        const due = new Date(t.due_date);
        due.setHours(0, 0, 0, 0);
        const diffDays = Math.floor((due.getTime() - now.getTime()) / 86400000);
        if (f.dueDateRange === "overdue" && diffDays >= 0) pass = false;
        if (f.dueDateRange === "today" && diffDays !== 0) pass = false;
        if (f.dueDateRange === "this-week" && (diffDays < 0 || diffDays > 6)) pass = false;
      }
    }

    if (pass) {
      matched.add(t.task_key);
    }
  }

  // Also include ancestors of matched tasks so the tree path is visible
  const taskMap = new Map<string, TaskSummary>();
  for (const t of tasks) taskMap.set(t.task_key, t);

  const visible = new Set(matched);
  for (const key of matched) {
    let current = taskMap.get(key);
    while (current?.parent_key) {
      visible.add(current.parent_key);
      current = taskMap.get(current.parent_key);
    }
  }

  return visible;
}

// Multi-select state
export const multiSelectedKeys = ref<Set<string>>(new Set());

export function isMultiSelected(key: string): boolean {
  return multiSelectedKeys.value.has(key);
}

export function toggleMultiSelect(key: string) {
  const s = new Set(multiSelectedKeys.value);
  if (s.has(key)) s.delete(key);
  else s.add(key);
  multiSelectedKeys.value = s;
}

export function clearMultiSelect() {
  multiSelectedKeys.value = new Set();
}

export function selectRange(keys: string[]) {
  multiSelectedKeys.value = new Set(keys);
}
