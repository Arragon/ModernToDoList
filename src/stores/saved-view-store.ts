/**
 * Saved Views (RD-M9-028~030).
 *
 * Views are disposable application state, never business data: the SQLite
 * `saved_views` table is the primary store, and a localStorage mirror keeps the
 * feature usable (and testable) while the backend command is still landing.
 */
import { computed, ref } from "vue";
import type { SavedViewDto, SavedViewWireDto, ViewPredicateDto } from "../ipc/types";
import * as ipc from "../ipc/client";
import { ipcFailureMessage } from "../ipc/safe";
import { applyFiltersWith, countMatches, filterState } from "./filter-state";
import type { FilterContext } from "./filter-state";
import { allTasks } from "./task-store";
import { participantsByTaskMap } from "./relation-store";
import {
  filterFromPredicates, predicatesFromFilter, predicateNodeFrom, predicatesFromNode,
} from "../app/view-predicates";
import { showToast, workspace } from "./app-state";

const STORAGE_KEY = "mtodo.saved-views.v1";

export const savedViews = ref<SavedViewDto[]>([]);
export const activeViewId = ref<string | null>(null);
export const savedViewsLoaded = ref(false);
/** True while the backend owns persistence; false when using the local mirror. */
export const usingBackendStore = ref(false);

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function readLocal(): SavedViewDto[] {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(isSavedView);
  } catch {
    return [];
  }
}

function isSavedView(value: unknown): value is SavedViewDto {
  if (!isRecord(value)) return false;
  return typeof value.id === "string" && typeof value.name === "string"
    && Array.isArray(value.predicates);
}

function writeLocal(views: SavedViewDto[]): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(views));
  } catch {
    // Non-fatal: views simply will not persist this run.
  }
}

function normalise(views: SavedViewDto[]): SavedViewDto[] {
  return views
    .map((view) => ({
      ...view,
      workspace_id: view.workspace_id ?? null,
      sort_order: view.sort_order ?? 0,
      count: view.count ?? null,
    }))
    .sort((a, b) => a.sort_order - b.sort_order || a.name.localeCompare(b.name));
}

/** Maps a backend saved-view row onto the flat in-memory representation. */
function fromWire(view: SavedViewWireDto): SavedViewDto {
  return {
    id: view.id,
    workspace_id: view.workspace_id,
    name: view.name,
    predicates: predicatesFromNode(view.predicates),
    sort_order: view.sort_order,
    count: null,
  };
}

export async function loadSavedViews(force = false): Promise<void> {
  if (savedViewsLoaded.value && !force) return;

  const workspaceId = workspace.value?.id;
  if (!workspaceId) {
    // No workspace open: the backend has no context to list views for, so use
    // the local mirror silently instead of reporting a failure.
    savedViews.value = normalise(readLocal());
    usingBackendStore.value = false;
    savedViewsLoaded.value = true;
    return;
  }

  const result = await ipc.listSavedViews(workspaceId);
  if (result.ok) {
    savedViews.value = normalise(result.value.map(fromWire));
    usingBackendStore.value = true;
    savedViewsLoaded.value = true;
    return;
  }

  savedViews.value = normalise(readLocal());
  usingBackendStore.value = false;
  savedViewsLoaded.value = true;
  if (!result.missing) {
    showToast(ipcFailureMessage(result, "Load saved views"), "warning");
  }
}

export function findView(viewId: string): SavedViewDto | undefined {
  return savedViews.value.find((v) => v.id === viewId);
}

export function makeViewId(): string {
  const random = Math.random().toString(36).slice(2, 10);
  return `view-${Date.now().toString(36)}-${random}`;
}

/** Creates a view from the current filter state. */
export async function createViewFromFilter(name: string): Promise<SavedViewDto | null> {
  const trimmed = name.trim();
  if (!trimmed) {
    showToast("A saved view needs a name", "warning");
    return null;
  }
  const predicates: ViewPredicateDto[] = predicatesFromFilter(filterState.value);
  if (predicates.length === 0) {
    showToast("Nothing to save: no filter conditions are active", "warning");
    return null;
  }

  const workspaceId = workspace.value?.id;
  const result = workspaceId
    ? await ipc.createSavedView(workspaceId, trimmed, predicateNodeFrom(predicates))
    : null;
  if (result?.ok) {
    const created = normalise([...savedViews.value, fromWire(result.value)]);
    savedViews.value = created;
    usingBackendStore.value = true;
    activeViewId.value = result.value.id;
    showToast(`Saved view "${trimmed}" created`, "success");
    return fromWire(result.value);
  }

  // Local mirror fallback so the feature degrades instead of failing outright.
  const view: SavedViewDto = {
    id: makeViewId(),
    workspace_id: null,
    name: trimmed,
    predicates,
    sort_order: savedViews.value.length,
    count: null,
  };
  savedViews.value = normalise([...savedViews.value, view]);
  usingBackendStore.value = false;
  writeLocal(savedViews.value);
  activeViewId.value = view.id;
  showToast(
    result === null
      ? `Saved view "${trimmed}" stored locally (no workspace open)`
      : result.missing
        ? `Saved view "${trimmed}" stored locally (backend command not available yet)`
        : `Saved view "${trimmed}" stored locally: ${result.error}`,
    "warning",
  );
  return view;
}

export async function renameView(viewId: string, name: string): Promise<boolean> {
  const trimmed = name.trim();
  if (!trimmed) {
    showToast("A saved view needs a name", "warning");
    return false;
  }
  const existing = findView(viewId);
  if (!existing) return false;

  if (usingBackendStore.value) {
    const result = await ipc.renameSavedView(viewId, trimmed);
    if (!result.ok) {
      showToast(ipcFailureMessage(result, "Rename saved view"), "warning");
      // Fall through to the local mirror so the rename is not lost.
      usingBackendStore.value = false;
    } else {
      savedViews.value = normalise(savedViews.value.map((v) =>
        v.id === viewId ? { ...v, name: trimmed } : v,
      ));
      return true;
    }
  }

  savedViews.value = normalise(savedViews.value.map((v) =>
    v.id === viewId ? { ...v, name: trimmed } : v,
  ));
  writeLocal(savedViews.value);
  return true;
}

export async function removeView(viewId: string): Promise<boolean> {
  if (usingBackendStore.value) {
    const result = await ipc.deleteSavedView(viewId);
    if (!result.ok) {
      showToast(ipcFailureMessage(result, "Delete saved view"), "warning");
      usingBackendStore.value = false;
    }
  }
  savedViews.value = normalise(savedViews.value.filter((v) => v.id !== viewId));
  if (activeViewId.value === viewId) activeViewId.value = null;
  if (!usingBackendStore.value) writeLocal(savedViews.value);
  return true;
}

/** Applies a saved view to the active filter state. */
export function applyView(viewId: string): boolean {
  const view = findView(viewId);
  if (!view) return false;
  const restored = filterFromPredicates(view.predicates);
  filterState.value = restored;
  activeViewId.value = viewId;
  return true;
}

export function clearActiveView(): void {
  activeViewId.value = null;
}

const filterContext = computed<FilterContext>(() => ({
  participantsByTask: participantsByTaskMap.value,
}));

/** Live result count per view, recomputed from the current task index. */
export const viewCounts = computed<Map<string, number>>(() => {
  const counts = new Map<string, number>();
  const tasks = allTasks.value;
  const context = filterContext.value;
  for (const view of savedViews.value) {
    const state = filterFromPredicates(view.predicates);
    counts.set(view.id, countMatches(tasks, state, context));
  }
  return counts;
});

export function countFor(viewId: string): number {
  return viewCounts.value.get(viewId) ?? 0;
}

/** True when the active filter matches a saved view's predicate set. */
export function matchesActiveFilter(view: SavedViewDto): boolean {
  const state = filterFromPredicates(view.predicates);
  const tasks = allTasks.value;
  const context = filterContext.value;
  const a = applyFiltersWith(state, tasks, context);
  const b = applyFiltersWith(filterState.value, tasks, context);
  if (a.size !== b.size) return false;
  for (const key of a) {
    if (!b.has(key)) return false;
  }
  return true;
}
