import { ref, computed } from "vue";
import type { TaskSummary } from "../ipc/types";

/** Grouping mode for the task tree (RD-M6-008 participant grouping). */
export type GroupMode = "none" | "participant";

export interface FilterState {
  titleKeyword: string;
  tags: string[];
  categories: string[];
  /** RD-M6-007: participant filter condition. */
  participants: string[];
  /** "any" = OR across selected participants, "all" = AND. */
  participantMode: "any" | "all";
  dueDateRange: "all" | "overdue" | "today" | "this-week" | "no-date";
  statusFilter: string | null;
  /** RD-M6-008: how the tree groups its rows. */
  groupBy: GroupMode;
}

function emptyFilter(): FilterState {
  return {
    titleKeyword: "",
    tags: [],
    categories: [],
    participants: [],
    participantMode: "any",
    dueDateRange: "all",
    statusFilter: null,
    groupBy: "none",
  };
}

export const filterState = ref<FilterState>(emptyFilter());

export function hasActiveConditions(f: FilterState): boolean {
  return (
    f.titleKeyword.length > 0 ||
    f.tags.length > 0 ||
    f.categories.length > 0 ||
    f.participants.length > 0 ||
    f.dueDateRange !== "all" ||
    f.statusFilter !== null
  );
}

export const isFilterActive = computed(() => hasActiveConditions(filterState.value));

export function clearFilters() {
  filterState.value = { ...emptyFilter(), groupBy: filterState.value.groupBy };
}

/** Relation lookups supplied by the caller so this module stays dependency-free. */
export interface FilterContext {
  /** task_key -> participant display names */
  participantsByTask?: Map<string, string[]>;
  /** task_key -> tags/categories */
  tagsByTask?: Map<string, string[]>;
}

/** Apply filters to a task list, returning matching task keys plus ancestors. */
export function applyFilters(tasks: TaskSummary[], context: FilterContext = {}): Set<string> {
  return applyFiltersWith(filterState.value, tasks, context);
}

/**
 * Pure variant used by saved views to count matches without mutating the
 * active filter state.
 */
export function applyFiltersWith(
  f: FilterState,
  tasks: TaskSummary[],
  context: FilterContext = {},
): Set<string> {
  const matched = collectMatches(f, tasks, context);
  if (!hasActiveConditions(f)) return matched;

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

/** Tasks that satisfy the conditions themselves, without ancestor padding. */
export function collectMatches(
  f: FilterState,
  tasks: TaskSummary[],
  context: FilterContext = {},
): Set<string> {
  if (!hasActiveConditions(f)) {
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

    // Tag / category filter
    if (pass && (f.tags.length > 0 || f.categories.length > 0)) {
      const wanted = new Set([...f.tags, ...f.categories].map((v) => v.toLowerCase()));
      const owned = context.tagsByTask?.get(t.task_key) ?? [];
      const hits = owned.filter((tag) => wanted.has(tag.toLowerCase())).length;
      if (hits < wanted.size) pass = false;
    }

    // Participant filter (RD-M6-007)
    if (pass && f.participants.length > 0) {
      const owned = (context.participantsByTask?.get(t.task_key) ?? []).map((p) => p.toLowerCase());
      const wanted = f.participants.map((p) => p.toLowerCase());
      const hits = wanted.filter((name) => owned.includes(name)).length;
      pass = f.participantMode === "all" ? hits === wanted.length : hits > 0;
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

  return matched;
}

/** Number of tasks satisfying a filter (no ancestor padding). */
export function countMatches(
  tasks: TaskSummary[],
  state: FilterState = filterState.value,
  context: FilterContext = {},
): number {
  return collectMatches(state, tasks, context).size;
}

// -- Participant filter helpers --
export function toggleParticipantFilter(name: string): void {
  const current = filterState.value.participants;
  filterState.value.participants = current.includes(name)
    ? current.filter((p) => p !== name)
    : [...current, name];
}

export function setParticipantFilter(names: string[]): void {
  filterState.value.participants = [...names];
}

export function clearParticipantFilter(): void {
  filterState.value.participants = [];
}

export function setGroupMode(mode: GroupMode): void {
  filterState.value.groupBy = mode;
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
