<script setup lang="ts">
/**
 * Quick Add (RD-M9-035~041).
 *
 *   Task title #tag @participant !priority due:2024-01-15
 *
 * The grammar is parsed client-side (`app/quick-add-parser.ts`) and previewed
 * live; unrecognised tokens stay part of the title. Submitting prefers the
 * backend `quick_add_task` command (which re-parses authoritatively and creates
 * the task inside the session) and falls back to the local parse + `add_task`
 * when that command is not registered yet.
 */
import { computed, ref, watch } from "vue";
import { parseQuickAdd } from "../../app/quick-add-parser";
import * as ipc from "../../ipc/client";
import { ipcFailureMessage } from "../../ipc/safe";
import { allTasks, createTask, getTask, jumpToTask, loadTasks, selectedTask } from "../../stores/task-store";
import { selectedTaskKey, showToast } from "../../stores/app-state";
import { quickAddFocusRequest } from "../../stores/ui-store";
import { ensureSessionForDocument } from "../../stores/session-store";
import { loadTaskParticipants } from "../../stores/relation-store";
import { isCommandAvailable } from "../../stores/capability-store";
import { Commands } from "../../ipc/commands";

const input = ref("");
const inputEl = ref<HTMLInputElement | null>(null);
const busy = ref(false);
/** False = create at the document root, true = create under the selection. */
const useSelectionAsParent = ref(true);

const parsed = computed(() => parseQuickAdd(input.value));
const hasText = computed(() => parsed.value.title.length > 0);

const parentTask = computed(() => {
  if (!useSelectionAsParent.value) return null;
  const key = selectedTaskKey.value;
  return key ? getTask(key) ?? null : null;
});

const targetDocumentId = computed<string | null>(() => {
  return parentTask.value?.document_id
    ?? selectedTask.value?.document_id
    ?? allTasks.value[0]?.document_id
    ?? null;
});

const targetLabel = computed(() =>
  parentTask.value
    ? `under “${parentTask.value.title || parentTask.value.task_key}”`
    : "at document root",
);

const priorityLabel = computed(() => {
  const value = parsed.value.priority;
  if (value < 0) return null;
  return ["None", "Low", "Medium", "High", "Very High"][value] ?? String(value);
});

watch(quickAddFocusRequest, async () => {
  await focus();
});

async function focus(): Promise<void> {
  inputEl.value?.focus();
  inputEl.value?.select();
}

function toggleTarget(): void {
  useSelectionAsParent.value = !useSelectionAsParent.value;
}

async function submit(): Promise<void> {
  const result = parsed.value;
  if (!hasText.value || busy.value) return;

  const documentId = targetDocumentId.value;
  if (!documentId) {
    showToast("Open a workspace with at least one document first", "warning");
    return;
  }

  busy.value = true;
  try {
    const parentKey = parentTask.value?.task_key ?? null;
    const created = await createViaBackend(result.title, documentId, parentKey)
      ?? await createViaAddTask(result, documentId, parentKey);

    if (created) {
      input.value = "";
      jumpToTask(created);
      await loadTaskParticipants({ task_key: created, document_id: documentId }, true);
      showToast(`Added “${result.title}” ${targetLabel.value}`, "success");
    }
  } finally {
    busy.value = false;
  }
}

/** Preferred path: the backend parses the raw grammar and creates the task. */
async function createViaBackend(
  rawTitle: string,
  documentId: string,
  parentKey: string | null,
): Promise<string | null> {
  if (!isCommandAvailable(Commands.QUICK_ADD_TASK)) return null;
  const parsedInput = parsed.value;
  const result = await ipc.quickAddTask({
    document_id: documentId,
    parent_key: parentKey,
    title: rawTitle,
    tags: parsedInput.tags,
    participants: parsedInput.participants,
    priority: Math.max(0, parsedInput.priority),
    start_date: parsedInput.startDate,
    due_date: parsedInput.dueDate,
  });
  if (!result.ok) {
    if (!result.missing) showToast(ipcFailureMessage(result, "Quick add"), "error");
    return null;
  }
  if (!result.value.success) {
    showToast("Backend rejected the quick add request", "error");
    return null;
  }
  await loadTasks();
  return result.value.task_key;
}

/** Fallback: local parse + `add_task` through the document session. */
async function createViaAddTask(
  result: ReturnType<typeof parseQuickAdd>,
  documentId: string,
  parentKey: string | null,
): Promise<string | null> {
  const sessionId = await ensureSessionForDocument(documentId);
  if (sessionId === null) return null;
  return createTask({
    title: result.title,
    documentId,
    parentKey,
    priority: result.priority >= 0 ? result.priority : 0,
    tags: result.tags,
    participants: result.participants,
    dueDate: result.dueDate,
    startDate: result.startDate,
  });
}

function onKeydown(e: KeyboardEvent): void {
  if (e.key === "Enter") {
    e.preventDefault();
    void submit();
  } else if (e.key === "Escape") {
    e.preventDefault();
    input.value = "";
    inputEl.value?.blur();
  }
}
</script>

<template>
  <div class="quick-add">
    <div class="quick-add__row">
      <i class="fas fa-bolt quick-add__icon"></i>
      <input
        ref="inputEl"
        v-model="input"
        class="quick-add__input"
        type="text"
        spellcheck="false"
        placeholder="Quick add: Fix login bug #auth @jane !high due:2024-01-15"
        aria-label="Quick add task"
        :disabled="busy"
        @keydown="onKeydown"
      />
      <button
        class="quick-add__target"
        :title="`Create ${targetLabel}. Click to toggle.`"
        @click="toggleTarget"
      >
        <i :class="parentTask ? 'fas fa-sitemap' : 'fas fa-folder-tree'"></i>
        {{ parentTask ? "subtask" : "root" }}
      </button>
      <button
        class="btn btn-primary quick-add__submit"
        :disabled="!hasText || busy"
        @click="submit"
      >
        <i :class="busy ? 'fas fa-spinner fa-spin' : 'fas fa-plus'"></i>
        Add
      </button>
    </div>

    <div v-if="input.trim()" class="quick-add__preview">
      <span class="quick-add__preview-title">{{ parsed.title || "(no title)" }}</span>
      <span v-for="tag in parsed.tags" :key="`tag-${tag}`" class="chip">
        <i class="fas fa-hashtag"></i>{{ tag }}
      </span>
      <span v-for="who in parsed.participants" :key="`p-${who}`" class="chip">
        <i class="fas fa-user"></i>{{ who }}
      </span>
      <span v-if="priorityLabel" class="chip">
        <i class="fas fa-flag"></i>{{ priorityLabel }}
      </span>
      <span v-if="parsed.dueDate" class="chip">
        <i class="fas fa-calendar-day"></i>due {{ parsed.dueDate }}
      </span>
      <span v-if="parsed.startDate" class="chip">
        <i class="fas fa-calendar"></i>start {{ parsed.startDate }}
      </span>
      <span v-if="parsed.percentDone >= 0" class="chip">
        <i class="fas fa-percent"></i>{{ parsed.percentDone }}%
      </span>
      <span class="quick-add__target-hint">{{ targetLabel }}</span>
    </div>
  </div>
</template>

<style scoped>
.quick-add {
  border-top: 1px solid var(--color-border);
  background: var(--color-bg-secondary);
  padding: var(--space-2) var(--space-3);
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}
.quick-add__row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}
.quick-add__icon {
  color: var(--color-accent);
  font-size: var(--text-sm);
  flex-shrink: 0;
}
.quick-add__input {
  flex: 1;
  min-width: 0;
  padding: var(--space-1) var(--space-2);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: var(--color-bg-primary);
  color: var(--color-text-primary);
  font-size: var(--text-sm);
  outline: none;
}
.quick-add__input:focus { border-color: var(--color-border-focus); }
.quick-add__input:disabled { color: var(--color-text-disabled); }
.quick-add__target {
  display: inline-flex;
  align-items: center;
  gap: var(--space-1);
  padding: var(--space-1) var(--space-2);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-full);
  background: var(--color-bg-primary);
  color: var(--color-text-secondary);
  font-size: var(--text-xs);
  cursor: pointer;
  flex-shrink: 0;
}
.quick-add__target:hover {
  border-color: var(--color-border-hover);
  color: var(--color-text-primary);
}
.quick-add__submit { flex-shrink: 0; }
.quick-add__submit:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.quick-add__preview {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-1);
  font-size: var(--text-xs);
  color: var(--color-text-muted);
}
.quick-add__preview-title {
  color: var(--color-text-secondary);
  font-weight: 600;
  max-width: 40%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.quick-add__target-hint {
  margin-left: auto;
  font-style: italic;
}
</style>
