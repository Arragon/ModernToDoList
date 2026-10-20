/**
 * Smart View definitions and client-side evaluation (RD-M9-014~021).
 *
 * The backend `get_smart_view` command is the authoritative evaluator (it
 * reads the workspace index and is covered by QA-M9-006~010 / QA-M10-G).
 * This module mirrors those semantics exactly so the sidebar can show live
 * counts and keep working when the backend command is unavailable
 * (degraded mode). Any change here must stay in sync with
 * `src-tauri/src/domain/smart_view.rs`.
 *
 * Smart views are read-only projections: they never mutate task data and
 * never persist anything beyond the active selection.
 */
import type { TaskSummary } from "../ipc/types";
import type { SmartViewKind } from "../ipc/types";

export interface SmartViewDef {
  kind: SmartViewKind;
  /** Sidebar label. */
  label: string;
  /** Font Awesome icon class. */
  icon: string;
  /** One-line explanation shown as the item tooltip. */
  hint: string;
  /** Grouping views delegate to group headers instead of a flat key set. */
  grouping: boolean;
}

/** Backend registry order (SmartViewKind) with UI metadata. */
export const SMART_VIEWS: SmartViewDef[] = [
  {
    kind: "today",
    label: "Today",
    icon: "fa-calendar-day",
    hint: "Unfinished tasks due today or already started",
    grouping: false,
  },
  {
    kind: "upcoming",
    label: "Upcoming",
    icon: "fa-calendar-week",
    hint: "Unfinished tasks due today or later (past-due live in Overdue)",
    grouping: false,
  },
  {
    kind: "overdue",
    label: "Overdue",
    icon: "fa-calendar-xmark",
    hint: "Unfinished tasks past their due date",
    grouping: false,
  },
  {
    kind: "unscheduled",
    label: "Unscheduled",
    icon: "fa-inbox",
    hint: "Unfinished tasks without start or due dates",
    grouping: false,
  },
  {
    kind: "completed",
    label: "Completed",
    icon: "fa-check-square",
    hint: "Finished tasks, most recently completed first",
    grouping: false,
  },
  {
    kind: "flagged",
    label: "Flagged",
    icon: "fa-flag",
    hint: "High priority (7+) tasks",
    grouping: false,
  },
  {
    kind: "by_participant",
    label: "By Participant",
    icon: "fa-users",
    hint: "Group tasks by participant",
    grouping: true,
  },
  {
    kind: "by_tag",
    label: "By Tag",
    icon: "fa-tags",
    hint: "Group tasks by tag",
    grouping: true,
  },
];

export const UNASSIGNED_GROUP = "(unassigned)";
export const UNTAGGED_GROUP = "(untagged)";

/** Mirrors the backend `FLAGGED_PRIORITY_THRESHOLD` (0-10 TDL scale). */
export const FLAGGED_PRIORITY_THRESHOLD = 7;

export function smartViewDef(kind: SmartViewKind): SmartViewDef | undefined {
  return SMART_VIEWS.find((v) => v.kind === kind);
}

// ── Date helpers (local calendar dates, mirroring backend NaiveDate) ─────────

/** Local calendar date as "YYYY-MM-DD" (never UTC-shifted). */
export function localDateStr(date: Date): string {
  const y = date.getFullYear();
  const m = String(date.getMonth() + 1).padStart(2, "0");
  const d = String(date.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}`;
}

export function todayStr(now: Date = new Date()): string {
  return localDateStr(now);
}

function parseDate(value: string | null): string | null {
  if (!value) return null;
  // Accept "YYYY-MM-DD" and full ISO timestamps; compare by calendar date.
  const head = value.slice(0, 10);
  return /^\d{4}-\d{2}-\d{2}$/.test(head) ? head : null;
}

function addDays(dateStr: string, days: number): string {
  const d = new Date(`${dateStr}T00:00:00`);
  d.setDate(d.getDate() + days);
  return localDateStr(d);
}

/** Monday-based week end (Sunday). Mirrors backend `end_of_week`. */
export function endOfWeek(today: string): string {
  const d = new Date(`${today}T00:00:00`);
  const dow = d.getDay(); // Sun=0 .. Sat=6
  const daysToSunday = dow === 0 ? 0 : 7 - dow;
  return addDays(today, daysToSunday);
}

// ── Status predicates (mirror backend ViewTask) ──────────────────────────────

export function isCompleted(task: TaskSummary): boolean {
  return (
    task.percent_done >= 100 ||
    task.status === "Completed" ||
    task.status === "Done"
  );
}

export function isCancelled(task: TaskSummary): boolean {
  return task.status === "Cancelled" || task.status === "Canceled";
}

export function isActive(task: TaskSummary): boolean {
  return !isCompleted(task) && !isCancelled(task);
}

// ── Evaluation ───────────────────────────────────────────────────────────────

export interface SmartViewContext {
  /** task_key -> participant display names. */
  participantsByTask?: Map<string, string[]>;
  /** task_key -> tags. */
  tagsByTask?: Map<string, string[]>;
}

/**
 * Keys of the tasks a flat smart view selects, mirroring the backend
 * evaluation order-insensitively (the tree applies its own display order).
 */
export function evaluateSmartViewKeys(
  kind: SmartViewKind,
  tasks: TaskSummary[],
  today: string = todayStr(),
): Set<string> {
  const matched = new Set<string>();
  for (const t of tasks) {
    if (matchesSmartView(kind, t, today)) matched.add(t.task_key);
  }
  return matched;
}

/** Whether a single task belongs to a flat smart view. */
export function matchesSmartView(
  kind: SmartViewKind,
  task: TaskSummary,
  today: string = todayStr(),
): boolean {
  switch (kind) {
    case "today": {
      if (!isActive(task)) return false;
      const due = parseDate(task.due_date);
      const start = parseDate(task.start_date);
      return due === today || (start !== null && start <= today);
    }
    case "upcoming": {
      if (!isActive(task)) return false;
      const due = parseDate(task.due_date);
      return due !== null && due >= today;
    }
    case "overdue": {
      if (!isActive(task)) return false;
      const due = parseDate(task.due_date);
      return due !== null && due < today;
    }
    case "unscheduled":
      return (
        isActive(task) &&
        parseDate(task.start_date) === null &&
        parseDate(task.due_date) === null
      );
    case "completed":
      return isCompleted(task);
    case "flagged":
      return task.priority >= FLAGGED_PRIORITY_THRESHOLD;
    case "by_participant":
    case "by_tag":
      // Grouping views select everything; grouping happens in the tree/panel.
      return true;
  }
}

export interface SmartViewGroup {
  name: string;
  taskKeys: string[];
}

/**
 * Groups for the grouping views (By Participant / By Tag), mirroring the
 * backend `group_by`: a task with several keys appears in each group,
 * keyless tasks land in the catch-all group, groups sorted by name with
 * the catch-all last.
 */
export function evaluateSmartViewGroups(
  kind: SmartViewKind,
  tasks: TaskSummary[],
  context: SmartViewContext = {},
): SmartViewGroup[] {
  const keysOf = (task: TaskSummary): string[] => {
    if (kind === "by_participant") {
      const names = context.participantsByTask?.get(task.task_key) ?? [];
      return names.length > 0 ? names : [UNASSIGNED_GROUP];
    }
    const tags = context.tagsByTask?.get(task.task_key) ?? [];
    return tags.length > 0 ? tags : [UNTAGGED_GROUP];
  };

  const groups = new Map<string, string[]>();
  for (const t of tasks) {
    for (const key of keysOf(t)) {
      const list = groups.get(key) ?? [];
      list.push(t.task_key);
      groups.set(key, list);
    }
  }

  const catchAll = kind === "by_participant" ? UNASSIGNED_GROUP : UNTAGGED_GROUP;
  return Array.from(groups.keys())
    .sort((a, b) => {
      if (a === catchAll) return 1;
      if (b === catchAll) return -1;
      return a.localeCompare(b);
    })
    .map((name) => ({ name, taskKeys: groups.get(name) ?? [] }));
}

/** Sidebar count badge: number of tasks a view selects. */
export function countSmartView(
  kind: SmartViewKind,
  tasks: TaskSummary[],
  context: SmartViewContext = {},
  today: string = todayStr(),
): number {
  if (kind === "by_participant" || kind === "by_tag") {
    // Distinct tasks covered by at least one group.
    const keys = new Set<string>();
    for (const g of evaluateSmartViewGroups(kind, tasks, context)) {
      for (const k of g.taskKeys) keys.add(k);
    }
    return keys.size;
  }
  return evaluateSmartViewKeys(kind, tasks, today).size;
}
