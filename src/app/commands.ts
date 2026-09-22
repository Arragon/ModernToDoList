/**
 * Application command registry (RD-M9-031~034).
 *
 * Every user-facing action is a command with an id, label, icon, shortcut and
 * category. The Ctrl+K palette, the keyboard shortcut registry and the sidebar
 * buttons all resolve through `executeCommand`, so a command that cannot run
 * (no workspace, disabled control, missing backend command) is reported in
 * exactly one place.
 */
import {
  workspace, documents, selectedTaskKey, expandedTaskKeys, closeWorkspace, scanAndIndex,
  rebuildIndex, loadWorkspace, openWorkspace, showConfirm, showToast, promptForText,
} from "../stores/app-state";
import {
  allTasks, createTask, deleteTask, jumpToTask, loadTasks, toggleTaskStatus,
} from "../stores/task-store";
import { clearFilters, clearMultiSelect, filterState, multiSelectedKeys, setGroupMode } from "../stores/filter-state";
import {
  canRedo, canUndo, closeAllSessions, redoActiveSession, saveActiveSession,
  saveSession, sessionIdForPath, setActiveDocument, undoActiveSession,
} from "../stores/session-store";
import { settings } from "../stores/settings-store";
import { requestQuickAddFocus, toggleCommandPalette, toggleGlobalSearch } from "../stores/ui-store";
import { createViewFromFilter, loadSavedViews } from "../stores/saved-view-store";
import { loadDependencies, loadParticipants, bulkAssignParticipants } from "../stores/relation-store";
import { fileNameOf, pickPath } from "./platform";
import * as ipc from "../ipc/client";
import type { TaskSummary } from "../ipc/types";
import { isCommandAvailable } from "../stores/capability-store";
import { Commands } from "../ipc/commands";

export type CommandCategory =
  | "workspace"
  | "document"
  | "task"
  | "edit"
  | "view"
  | "search"
  | "performance";

export interface AppCommand {
  id: string;
  label: string;
  icon?: string;
  shortcut?: string;
  category: CommandCategory;
  /** Optional hint rendered next to the label in the palette. */
  description?: string;
  enabled: () => boolean;
  execute: () => void | Promise<void>;
}

export interface ExecuteOptions {
  /** Suppress the "not available" toast (used by global keyboard shortcuts). */
  silentWhenDisabled?: boolean;
}

const commands = new Map<string, AppCommand>();

export function registerCommand(cmd: AppCommand) {
  commands.set(cmd.id, cmd);
}

export function getCommand(id: string): AppCommand | undefined {
  return commands.get(id);
}

/**
 * Runs a command. Returns false when it is unknown or disabled; async failures
 * are surfaced as a toast so no unhandled rejection can escape.
 */
export function executeCommand(id: string, options: ExecuteOptions = {}): boolean {
  const cmd = commands.get(id);
  if (!cmd) {
    showToast(`Unknown command: ${id}`, "warning");
    return false;
  }
  if (!cmd.enabled()) {
    if (!options.silentWhenDisabled) {
      showToast(`"${cmd.label}" is not available right now`, "warning");
    }
    return false;
  }
  try {
    const result = cmd.execute();
    if (result instanceof Promise) {
      result.catch((err: unknown) => {
        showToast(`${cmd.label} failed: ${describe(err)}`, "error");
      });
    }
  } catch (err) {
    showToast(`${cmd.label} failed: ${describe(err)}`, "error");
    return false;
  }
  return true;
}

export function getAllCommands(): AppCommand[] {
  return Array.from(commands.values());
}

export function enabledCommands(): AppCommand[] {
  return getAllCommands().filter((c) => c.enabled());
}

function describe(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  return String(err);
}

const hasWorkspace = () => workspace.value !== null;
const hasDocument = () => hasWorkspace() && documents.value.length > 0;
const hasSelection = () => hasDocument() && selectedTaskKey.value !== null;

// ── Workspace ────────────────────────────────────────────────────────────────

registerCommand({
  id: "workspace.open",
  label: "Open Workspace…",
  icon: "fa-folder-open",
  category: "workspace",
  description: "Pick a folder and load its task documents",
  enabled: () => true,
  execute: async () => {
    const path = await pickPath({
      title: "Open Workspace",
      mode: "directory",
      message: "Choose the folder that holds your task documents.",
      placeholder: "D:\\Projects\\MyWorkspace",
    });
    if (!path) return;
    await loadWorkspace(path);
    if (workspace.value === null) {
      const name = fileNameOf(path) || "Workspace";
      showConfirm(
        "Create workspace?",
        `"${path}" is not a workspace yet. Create one there?`,
        () => { void openWorkspace(path, name); },
        "Create",
      );
      return;
    }
    await afterWorkspaceOpened();
  },
});

registerCommand({
  id: "workspace.create",
  label: "New Workspace…",
  icon: "fa-folder-plus",
  category: "workspace",
  description: "Create a workspace folder and index it",
  enabled: () => true,
  execute: async () => {
    const path = await pickPath({
      title: "New Workspace",
      mode: "directory",
      message: "Choose where the new workspace folder lives.",
      placeholder: "D:\\Projects\\MyWorkspace",
    });
    if (!path) return;
    const name = fileNameOf(path) || "Workspace";
    await openWorkspace(path, name);
    if (workspace.value !== null) await afterWorkspaceOpened();
  },
});

registerCommand({
  id: "workspace.close",
  label: "Close Workspace",
  icon: "fa-folder-minus",
  category: "workspace",
  enabled: () => hasWorkspace(),
  execute: async () => {
    await closeAllSessions();
    await closeWorkspace();
    clearFilters();
    clearMultiSelect();
  },
});

registerCommand({
  id: "workspace.scanAndIndex",
  label: "Scan & Index",
  icon: "fa-magnifying-glass",
  category: "workspace",
  enabled: () => hasWorkspace(),
  execute: () => scanAndIndex(),
});

registerCommand({
  id: "workspace.rebuildIndex",
  label: "Rebuild Index",
  icon: "fa-arrows-rotate",
  category: "workspace",
  enabled: () => hasWorkspace(),
  execute: async () => {
    await rebuildIndex();
    await loadTasks();
    await refreshRelations();
  },
});

// ── Documents / sessions ─────────────────────────────────────────────────────

registerCommand({
  id: "file.save",
  label: "Save",
  icon: "fa-floppy-disk",
  shortcut: "Ctrl+S",
  category: "document",
  description: "Atomically save the active document session",
  enabled: () => hasDocument(),
  execute: async () => {
    await ensureActiveDocument();
    await saveActiveSession();
  },
});

registerCommand({
  id: "file.saveAll",
  label: "Save All Open Documents",
  icon: "fa-layer-group",
  shortcut: "Ctrl+Shift+S",
  category: "document",
  enabled: () => hasDocument(),
  execute: async () => {
    let saved = 0;
    for (const doc of documents.value) {
      const sessionId = sessionIdForPath(doc.file_path);
      if (sessionId === null) continue;
      if (await saveSession(sessionId)) saved += 1;
    }
    showToast(
      saved > 0 ? `Saved ${saved} document(s)` : "No open document sessions to save",
      saved > 0 ? "success" : "info",
    );
  },
});

registerCommand({
  id: "edit.undo",
  label: "Undo",
  icon: "fa-rotate-left",
  shortcut: "Ctrl+Z",
  category: "edit",
  enabled: () => hasDocument() && canUndo.value,
  execute: async () => {
    const changed = await undoActiveSession();
    if (changed) await loadTasks();
  },
});

registerCommand({
  id: "edit.redo",
  label: "Redo",
  icon: "fa-rotate-right",
  shortcut: "Ctrl+Y",
  category: "edit",
  enabled: () => hasDocument() && canRedo.value,
  execute: async () => {
    const changed = await redoActiveSession();
    if (changed) await loadTasks();
  },
});

// ── Tasks ────────────────────────────────────────────────────────────────────

registerCommand({
  id: "task.addRoot",
  label: "Add Root Task",
  icon: "fa-plus",
  shortcut: "Ctrl+N",
  category: "task",
  description: "Create a task at the document root",
  enabled: () => hasDocument() && isCommandAvailable(Commands.ADD_TASK),
  execute: () => addTaskFlow(null),
});

registerCommand({
  id: "task.addChild",
  label: "Add Subtask",
  icon: "fa-sitemap",
  shortcut: "Ctrl+Shift+N",
  category: "task",
  description: "Create a task under the current selection",
  enabled: () => hasSelection() && isCommandAvailable(Commands.ADD_TASK),
  execute: () => addTaskFlow(selectedTaskKey.value),
});

registerCommand({
  id: "task.delete",
  label: "Delete Task",
  icon: "fa-trash",
  shortcut: "Delete",
  category: "task",
  enabled: () => hasSelection(),
  execute: () => {
    const key = selectedTaskKey.value;
    if (!key) return;
    showConfirm(
      "Delete task?",
      "The task and its relations are removed from the document. This can be undone.",
      () => { void deleteTask(key).then(() => loadTasks()); },
      "Delete",
    );
  },
});

registerCommand({
  id: "task.toggleComplete",
  label: "Toggle Completion",
  icon: "fa-circle-check",
  shortcut: "Space",
  category: "task",
  enabled: () => hasSelection(),
  execute: async () => {
    const key = selectedTaskKey.value;
    if (!key) return;
    await toggleTaskStatus(key);
  },
});

registerCommand({
  id: "task.quickAdd",
  label: "Quick Add Task",
  icon: "fa-bolt",
  shortcut: "Ctrl+Enter",
  category: "task",
  description: "Focus the Quick Add bar (title #tag @who !priority due:YYYY-MM-DD)",
  enabled: () => hasDocument(),
  execute: () => requestQuickAddFocus(),
});

registerCommand({
  id: "task.assignParticipants",
  label: "Assign Participants to Selection…",
  icon: "fa-user-plus",
  category: "task",
  description: "Bulk participant assignment (RD-M6-006)",
  enabled: () => hasSelection(),
  execute: async () => {
    const targets = bulkTargetTasks();
    const names = await promptForText({
      title: "Assign participants",
      message: `Comma-separated names for ${targets.length} task(s).`,
      placeholder: "Jane Doe, John Smith",
    });
    if (!names) return;
    const list = names.split(",").map((n) => n.trim()).filter(Boolean);
    if (list.length === 0) return;
    const tasks = bulkTargetTasks();
    const result = await bulkAssignParticipants(tasks, list, false);
    showToast(
      `Added ${result.added} participant assignment(s), ${result.failed} failed`,
      result.failed > 0 ? "warning" : "success",
    );
  },
});

// ── Search / views ───────────────────────────────────────────────────────────

registerCommand({
  id: "view.commandPalette",
  label: "Command Palette",
  icon: "fa-terminal",
  shortcut: "Ctrl+K",
  category: "search",
  description: "Fuzzy search over commands and task titles",
  enabled: () => settings.commandPaletteEnabled,
  execute: () => toggleCommandPalette(),
});

registerCommand({
  id: "view.globalSearch",
  label: "Global Search",
  icon: "fa-magnifying-glass",
  shortcut: "Ctrl+F",
  category: "search",
  description: "Search tasks across the workspace",
  enabled: () => hasDocument(),
  execute: () => toggleGlobalSearch(),
});

registerCommand({
  id: "view.clearFilters",
  label: "Clear Filters",
  icon: "fa-filter-circle-xmark",
  category: "view",
  enabled: () => hasDocument(),
  execute: () => {
    clearFilters();
    showToast("Filters cleared", "info");
  },
});

registerCommand({
  id: "view.groupByParticipant",
  label: "Group by Participant",
  icon: "fa-users",
  category: "view",
  description: "Toggle the participant grouping view mode",
  enabled: () => hasDocument(),
  execute: async () => {
    const next = filterState.value.groupBy === "participant" ? "none" : "participant";
    if (next === "participant") await loadParticipants();
    setGroupMode(next);
  },
});

registerCommand({
  id: "view.expandAll",
  label: "Expand All Tasks",
  icon: "fa-angles-down",
  category: "view",
  enabled: () => hasDocument(),
  execute: () => {
    const keys = new Set<string>();
    for (const task of allTasks.value) keys.add(task.task_key);
    expandedTaskKeys.value = keys;
  },
});

registerCommand({
  id: "view.collapseAll",
  label: "Collapse All Tasks",
  icon: "fa-angles-up",
  category: "view",
  enabled: () => hasDocument(),
  execute: () => {
    expandedTaskKeys.value = new Set<string>();
  },
});

registerCommand({
  id: "view.jumpToSelection",
  label: "Scroll to Selected Task",
  icon: "fa-location-crosshairs",
  shortcut: "Ctrl+L",
  category: "view",
  enabled: () => hasSelection(),
  execute: () => {
    const key = selectedTaskKey.value;
    if (key) jumpToTask(key);
  },
});

registerCommand({
  id: "view.saveCurrentFilter",
  label: "Save Current Filter as View…",
  icon: "fa-bookmark",
  category: "view",
  description: "RD-M9-028: create from current filter state",
  enabled: () => hasDocument(),
  execute: async () => {
    const name = await promptForText({
      title: "Save View",
      message: "Name this view. It captures the active filter conditions.",
      placeholder: "My open high-priority work",
    });
    if (!name) return;
    await createViewFromFilter(name);
  },
});

registerCommand({
  id: "view.reloadSavedViews",
  label: "Reload Saved Views",
  icon: "fa-rotate",
  category: "view",
  enabled: () => hasWorkspace(),
  execute: () => loadSavedViews(true),
});

// ── Performance (RD-M10-022 gate controls) ───────────────────────────────────

registerCommand({
  id: "perf.virtualizationOn",
  label: "Force Tree Virtualization On",
  icon: "fa-list-ol",
  category: "performance",
  description: "RD-M10-023: render only visible rows + overscan buffer",
  enabled: () => settings.virtualization.mode !== "on",
  execute: () => {
    settings.virtualization.mode = "on";
    showToast("Tree virtualization forced ON", "success");
  },
});

registerCommand({
  id: "perf.virtualizationOff",
  label: "Force Tree Virtualization Off",
  icon: "fa-list",
  category: "performance",
  description: "RD-M10-022 SKIP path: render every row",
  enabled: () => settings.virtualization.mode !== "off",
  execute: () => {
    settings.virtualization.mode = "off";
    showToast("Tree virtualization forced OFF", "success");
  },
});

registerCommand({
  id: "perf.virtualizationAuto",
  label: "Tree Virtualization: Auto",
  icon: "fa-wand-magic-sparkles",
  category: "performance",
  description: "Enable automatically above the configured row threshold",
  enabled: () => settings.virtualization.mode !== "auto",
  execute: () => {
    settings.virtualization.mode = "auto";
    showToast("Tree virtualization set to AUTO", "success");
  },
});

registerCommand({
  id: "perf.dumpRenderMetrics",
  label: "Log Render Metrics",
  icon: "fa-gauge-high",
  category: "performance",
  description: "RD-M10-022 evidence: window.__mtodoPerf.summarize()",
  enabled: () => true,
  execute: () => {
    const api = typeof window !== "undefined" ? window.__mtodoPerf : undefined;
    const summary = api ? api.summarize() : [];
    if (summary.length === 0) {
      showToast("No render samples recorded yet", "info");
      return;
    }
    console.table(summary);
    showToast(`Logged ${summary.length} render metric group(s) to the console`, "success");
  },
});

// ── Helpers ──────────────────────────────────────────────────────────────────

/** Selection targets for bulk edits: multi-selection, else the focused task. */
export function bulkTargets(): string[] {
  const multi = Array.from(multiSelectedKeys.value);
  if (multi.length > 0) return multi;
  return selectedTaskKey.value ? [selectedTaskKey.value] : [];
}

/** Tasks targeted by bulk edits, resolved against the loaded index. */
export function bulkTargetTasks() {
  return bulkTargets()
    .map((key) => allTasks.value.find((t) => t.task_key === key))
    .filter((t): t is TaskSummary => Boolean(t));
}

/** Allocates a task id (M2 allocator) and creates the task through the session. */
async function addTaskFlow(parentKey: string | null): Promise<void> {
  const documentId = parentKey
    ? allTasks.value.find((t) => t.task_key === parentKey)?.document_id
      ?? documents.value[0]?.id ?? null
    : documents.value[0]?.id ?? null;
  if (!documentId) {
    showToast("No document is available to add a task to", "warning");
    return;
  }

  const allocated = await allocateTaskKey(documentId);
  const created = await createTask({
    title: "New Task",
    documentId,
    parentKey,
    priority: 0,
    tags: [],
    participants: [],
    dueDate: null,
    startDate: null,
    taskKey: allocated,
  });
  if (created) {
    setActiveDocument(documentId);
    jumpToTask(created);
    showToast("Task created — rename it in the Inspector", "success");
  }
}

/**
 * `task.addRoot` uses the M2 id allocator so the new TaskId never collides with
 * an existing id, then hands it to the session's add-task command.
 */
async function allocateTaskKey(documentId: string): Promise<string | null> {
  const doc = documents.value.find((d) => d.id === documentId);
  if (!doc) return null;
  try {
    const metadata = await ipc.getDocumentMetadata(doc.file_path);
    const existingIds = allTasks.value
      .filter((t) => t.document_id === documentId)
      .map((t) => t.task_key);
    const allocated = await ipc.allocateTaskId(existingIds, metadata.next_unique_id);
    return allocated.id;
  } catch (err) {
    // The backend may allocate the id itself; pre-allocation is an optimisation.
    console.warn("Could not pre-allocate a task id:", describe(err));
    return null;
  }
}

async function ensureActiveDocument(): Promise<void> {
  const key = selectedTaskKey.value;
  const task = key ? allTasks.value.find((t) => t.task_key === key) : undefined;
  if (task) {
    setActiveDocument(task.document_id);
    return;
  }
  if (documents.value.length > 0) setActiveDocument(documents.value[0].id);
}

async function afterWorkspaceOpened(): Promise<void> {
  // Index first: list_documents / query_tasks read the SQLite index, so without
  // a scan a freshly opened workspace shows no documents or tasks and task
  // creation has no document to attach to.
  await scanAndIndex();
  await loadTasks();
  await refreshRelations();
  await loadSavedViews(true);
}

async function refreshRelations(): Promise<void> {
  await Promise.all([loadParticipants(true), loadDependencies(true)]);
}
