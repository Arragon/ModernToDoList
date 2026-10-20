/**
 * Smart view store (RD-M9-014~021).
 *
 * Evaluation strategy: the backend `get_smart_view` command is authoritative
 * (it reads the workspace index and is covered by QA-M9-006~010 and
 * QA-M10-G). When the command is unavailable (older backend, no workspace
 * index) the store falls back to the client-side mirror in
 * `app/smart-views.ts`, which implements identical semantics over the
 * already-loaded task list.
 *
 * Smart views are projections only: selecting one never mutates task data,
 * and the only persisted state is the in-memory selection.
 */
import { computed, ref, watch } from "vue";
import * as ipc from "../ipc/client";
import type { SmartViewKind, SmartViewResponse } from "../ipc/types";
import {
  SMART_VIEWS, countSmartView, evaluateSmartViewGroups, evaluateSmartViewKeys,
  smartViewDef, todayStr,
} from "../app/smart-views";
import { allTasks } from "./task-store";
import { participantsByTaskMap } from "./relation-store";
import { tagsFor } from "./task-store";
import { showToast } from "./app-state";
import {
  activeSmartView, clearSmartViewSelection, smartViewGroupKeys, smartViewKeys,
} from "./smart-view-state";
import { setGroupMode } from "./filter-state";

/** Backend response of the active view (groups for grouping views). */
export const smartViewResponse = ref<SmartViewResponse | null>(null);

/** True while a view evaluation is in flight. */
export const smartViewLoading = ref(false);

/** Group rows for the active grouping view (by_tag drill-down in the panel). */
export const smartViewGroups = ref<{ name: string; taskKeys: string[] }[]>([]);

/** Expanded group name in the panel (by_tag). */
export const expandedSmartGroup = ref<string | null>(null);

const tasksWithTags = computed(() =>
  allTasks.value.map((t) => ({ task: t, tags: tagsFor(t.task_key) })),
);

/** Live standalone count per view (badge), independent of the active filter. */
export function smartViewCount(kind: SmartViewKind): number {
  const tagCtx = {
    participantsByTask: participantsByTaskMap.value,
    tagsByTask: new Map(tasksWithTags.value.map((r) => [r.task.task_key, r.tags])),
  };
  return countSmartView(kind, allTasks.value, tagCtx);
}

export const smartViewCounts = computed<Record<SmartViewKind, number>>(() => {
  const tagCtx = {
    participantsByTask: participantsByTaskMap.value,
    tagsByTask: new Map(tasksWithTags.value.map((r) => [r.task.task_key, r.tags])),
  };
  const out = {} as Record<SmartViewKind, number>;
  for (const def of SMART_VIEWS) out[def.kind] = countSmartView(def.kind, allTasks.value, tagCtx);
  return out;
});

/** True when the active view is a grouping view. */
export const activeViewIsGrouping = computed(() => {
  const kind = activeSmartView.value;
  return kind ? (smartViewDef(kind)?.grouping ?? false) : false;
});

/**
 * Selects (or toggles off) a smart view.
 *
 * Flat views project a key set into the task tree. `by_participant` reuses
 * the tree's participant grouping. `by_tag` loads authoritative groups from
 * the backend (client-side fallback over cached tags) and drills down per
 * group. Selecting the active view again clears it.
 */
export async function selectSmartView(kind: SmartViewKind): Promise<void> {
  const def = smartViewDef(kind);
  if (!def) return;

  if (activeSmartView.value === kind) {
    // Grouping views expand/collapse their group list; flat views clear.
    if (def.grouping && kind === "by_tag" && expandedSmartGroup.value === null) {
      expandedSmartGroup.value = "__all__";
      return;
    }
    clearSmartView();
    return;
  }

  clearSmartViewSelection();
  activeSmartView.value = kind;

  if (kind === "by_participant") {
    // The tree renders participant group headers; no key constraint.
    setGroupMode("participant");
  }

  smartViewLoading.value = true;
  try {
    const result = await ipc.getSmartView(kind);
    if (result.ok) {
      smartViewResponse.value = result.value;
      applyBackendResponse(kind, result.value);
      return;
    }
    if (!result.missing) {
      showToast(`Smart view failed: ${result.error}`, "warning");
    }
  } finally {
    smartViewLoading.value = false;
  }
  applyClientFallback(kind);
}

function applyBackendResponse(kind: SmartViewKind, response: SmartViewResponse): void {
  if (smartViewDef(kind)?.grouping) {
    smartViewGroups.value = response.groups.map((g) => ({
      name: g.name,
      taskKeys: g.tasks.map((t) => t.task_key),
    }));
    if (kind === "by_tag") expandedSmartGroup.value = "__all__";
    // Upcoming groups (flat bucketing) are not needed for the panel.
    return;
  }
  const keys = new Set(response.tasks.map((t) => t.task_key));
  smartViewKeys.value = keys;
}

function applyClientFallback(kind: SmartViewKind): void {
  const def = smartViewDef(kind);
  if (!def) return;
  if (def.grouping) {
    smartViewGroups.value = evaluateSmartViewGroups(kind, allTasks.value, {
      participantsByTask: participantsByTaskMap.value,
      tagsByTask: new Map(tasksWithTags.value.map((r) => [r.task.task_key, r.tags])),
    });
    if (kind === "by_tag") expandedSmartGroup.value = "__all__";
    return;
  }
  smartViewKeys.value = evaluateSmartViewKeys(kind, allTasks.value, todayStr());
}

/** Drills into one group of a grouping view (by_tag); clicking the active group returns to the overview. */
export function selectSmartGroup(name: string): void {
  const group = smartViewGroups.value.find((g) => g.name === name);
  if (!group) return;
  expandedSmartGroup.value = name;
  smartViewGroupKeys.value = new Set(group.taskKeys);
}

/** Returns to the full group overview (all groups visible, no key constraint). */
export function clearSmartGroup(): void {
  expandedSmartGroup.value = "__all__";
  smartViewGroupKeys.value = null;
}

export function clearSmartView(): void {
  const wasGroupingParticipant = activeSmartView.value === "by_participant";
  clearSmartViewSelection();
  smartViewResponse.value = null;
  smartViewGroups.value = [];
  expandedSmartGroup.value = null;
  if (wasGroupingParticipant) setGroupMode("none");
}

/** Re-evaluates the active view (called after index/task changes). */
export async function refreshSmartView(): Promise<void> {
  const kind = activeSmartView.value;
  if (!kind) return;

  if (smartViewGroupKeys.value) {
    // Drilled into one group: refresh the group membership from the backend
    // when available, otherwise re-derive from the current task list.
    const name = expandedSmartGroup.value;
    const result = await ipc.getSmartView(kind);
    if (result.ok) {
      smartViewResponse.value = result.value;
      smartViewGroups.value = result.value.groups.map((g) => ({
        name: g.name,
        taskKeys: g.tasks.map((t) => t.task_key),
      }));
      const group = smartViewGroups.value.find((g) => g.name === name);
      if (group) {
        smartViewGroupKeys.value = new Set(group.taskKeys);
        return;
      }
    }
    applyClientFallback(kind);
    const group = smartViewGroups.value.find((g) => g.name === name);
    if (group) {
      smartViewGroupKeys.value = new Set(group.taskKeys);
    } else {
      clearSmartGroup();
    }
    return;
  }

  const def = smartViewDef(kind);
  if (def?.grouping) {
    if (kind === "by_tag") applyClientFallback(kind);
    return;
  }
  const result = await ipc.getSmartView(kind);
  if (result.ok) {
    smartViewResponse.value = result.value;
    applyBackendResponse(kind, result.value);
  } else {
    applyClientFallback(kind);
  }
}

export const smartViewTotal = computed(() => {
  if (smartViewResponse.value) return smartViewResponse.value.total;
  if (smartViewKeys.value) return smartViewKeys.value.size;
  if (smartViewGroupKeys.value) return smartViewGroupKeys.value.size;
  return 0;
});

// Keep the active projection fresh when the task list changes (index scan,
// external modification, Quick Add, edits). Debounced by Vue's scheduler.
watch(allTasks, () => {
  if (activeSmartView.value) void refreshSmartView();
});
