/**
 * Saved View predicate model (RD-M9-022~027).
 *
 * `FilterState` is the canonical in-memory filter; saved views persist a
 * version-neutral list of `ViewPredicateDto`. The conversion is lossless for
 * every condition the UI can express, and unknown predicates are ignored when a
 * view is restored so a view saved by a newer build still loads.
 */
import type { ViewPredicateDto } from "../ipc/types";
import type { FilterState, GroupMode } from "../stores/filter-state";

export const PREDICATE_VERSION = 1;

const DUE_RANGES = new Set(["all", "overdue", "today", "this-week", "no-date"]);

export function predicatesFromFilter(state: FilterState): ViewPredicateDto[] {
  const predicates: ViewPredicateDto[] = [];

  if (state.titleKeyword.trim()) {
    predicates.push({ field: "title", operator: "contains", value: state.titleKeyword.trim() });
  }
  if (state.statusFilter) {
    predicates.push({ field: "status", operator: "eq", value: state.statusFilter });
  }
  if (state.tags.length > 0) {
    predicates.push({ field: "tag", operator: "in", value: [...state.tags] });
  }
  if (state.categories.length > 0) {
    predicates.push({ field: "tag", operator: "in", value: [...state.categories] });
  }
  if (state.participants.length > 0) {
    predicates.push({
      field: "participant",
      operator: state.participantMode === "all" ? "eq" : "in",
      value: [...state.participants],
    });
  }
  if (state.dueDateRange !== "all") {
    predicates.push({ field: "due_date", operator: "relative", value: state.dueDateRange });
  }
  if (state.groupBy !== "none") {
    predicates.push({ field: "group_by", operator: "eq", value: state.groupBy });
  }
  return predicates;
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

export function filterFromPredicates(predicates: ViewPredicateDto[]): FilterState {
  const state = emptyFilter();

  for (const predicate of predicates) {
    const { field, operator, value } = predicate;

    if (field === "title" && operator === "contains" && typeof value === "string") {
      state.titleKeyword = value;
      continue;
    }
    if (field === "status" && operator === "eq" && typeof value === "string") {
      state.statusFilter = value;
      continue;
    }
    if (field === "tag" && operator === "in" && Array.isArray(value)) {
      state.tags.push(...value.filter(isString));
      continue;
    }
    if (field === "participant") {
      const names = Array.isArray(value) ? value.filter(isString) : isString(value) ? [value] : [];
      state.participants.push(...names);
      state.participantMode = operator === "eq" ? "all" : "any";
      continue;
    }
    if (field === "due_date" && operator === "relative" && typeof value === "string") {
      if (DUE_RANGES.has(value)) {
        state.dueDateRange = value as FilterState["dueDateRange"];
      }
      continue;
    }
    if (field === "group_by" && typeof value === "string") {
      state.groupBy = value === "participant" ? ("participant" as GroupMode) : "none";
    }
  }

  state.tags = dedupe(state.tags);
  state.participants = dedupe(state.participants);
  return state;
}

/** Short human-readable summary shown under a saved view in the sidebar. */
export function describePredicates(predicates: ViewPredicateDto[]): string {
  if (predicates.length === 0) return "No conditions";
  return predicates.map(describePredicate).join(" · ");
}

function describePredicate(predicate: ViewPredicateDto): string {
  const value = Array.isArray(predicate.value)
    ? predicate.value.join(", ")
    : predicate.value === null
      ? "—"
      : String(predicate.value);

  switch (predicate.field) {
    case "title": return `title contains "${value}"`;
    case "status": return `status = ${value}`;
    case "tag": return `tags: ${value}`;
    case "participant":
      return predicate.operator === "eq" ? `participants: all of ${value}` : `participant: ${value}`;
    case "due_date": return `due: ${value}`;
    case "start_date": return `start: ${value}`;
    case "group_by": return `group by ${value}`;
    case "priority": return `priority ${predicate.operator} ${value}`;
    default: return `${predicate.field} ${predicate.operator} ${value}`;
  }
}

function isString(value: unknown): value is string {
  return typeof value === "string";
}

function dedupe(values: string[]): string[] {
  return Array.from(new Set(values));
}
