<script setup lang="ts">
/**
 * Inspector (Phase 0.1: every editor is now wired to the backend).
 *
 * Field edits go through `update_task_field` on the document session opened for
 * the selected task; relation editors (participants, dependencies, progress
 * links, attachments) manage their own calls. When a command is missing the
 * control renders disabled and a toast explains why — nothing throws.
 */
import { computed, ref, watch } from "vue";
import * as ipc from "../../ipc/client";
import { Commands } from "../../ipc/commands";
import { isCommandAvailable } from "../../stores/capability-store";
import { cacheTags, selectedTask, setTaskTags, tagsFor, updateTaskField } from "../../stores/task-store";
import { setActiveDocument } from "../../stores/session-store";
import TitleEditor from "./TitleEditor.vue";
import StatusEditor from "./StatusEditor.vue";
import PriorityEditor from "./PriorityEditor.vue";
import DateEditor from "./DateEditor.vue";
import TagsEditor from "./TagsEditor.vue";
import MoreProperties from "./MoreProperties.vue";
import ParticipantsEditor from "./ParticipantsEditor.vue";
import DependencyEditor from "./DependencyEditor.vue";
import ProgressLinksEditor from "./ProgressLinksEditor.vue";
import AttachmentListEditor from "./AttachmentListEditor.vue";
import EmptyState from "../common/EmptyState.vue";

const taskTags = ref<string[]>([]);

const task = computed(() => selectedTask.value);
const canEditField = computed(() => isCommandAvailable(Commands.UPDATE_TASK_FIELD));
const canEditTags = computed(() => isCommandAvailable(Commands.SET_TASK_TAGS));

watch(task, async (current) => {
  if (!current) {
    taskTags.value = [];
    return;
  }
  setActiveDocument(current.document_id);
  const cached = tagsFor(current.task_key);
  taskTags.value = cached;
  try {
    const fresh = await ipc.getTaskTags(current.task_key);
    taskTags.value = fresh;
    cacheTags(current.task_key, fresh);
  } catch {
    // Tag lookup is best-effort; the cached value (possibly empty) is kept.
    taskTags.value = cached;
  }
}, { immediate: true });

function key(): string | null {
  return task.value?.task_key ?? null;
}

async function onTitleUpdate(value: string): Promise<void> {
  const taskKey = key();
  if (!taskKey) return;
  if (task.value?.title === value) return;
  await updateTaskField(taskKey, "title", value);
}

async function onStatusUpdate(value: string): Promise<void> {
  const taskKey = key();
  if (!taskKey) return;
  await updateTaskField(taskKey, "status", value);
}

async function onPriorityUpdate(value: number): Promise<void> {
  const taskKey = key();
  if (!taskKey) return;
  await updateTaskField(taskKey, "priority", String(value));
}

async function onStartDateUpdate(value: string | null): Promise<void> {
  const taskKey = key();
  if (!taskKey) return;
  await updateTaskField(taskKey, "start_date", value ?? "");
}

async function onDueDateUpdate(value: string | null): Promise<void> {
  const taskKey = key();
  if (!taskKey) return;
  await updateTaskField(taskKey, "due_date", value ?? "");
}

async function onCompletedDateUpdate(value: string | null): Promise<void> {
  const taskKey = key();
  if (!taskKey) return;
  await updateTaskField(taskKey, "completed_date", value ?? "");
}

async function onPercentDoneUpdate(value: number): Promise<void> {
  const taskKey = key();
  if (!taskKey) return;
  await updateTaskField(taskKey, "percent_done", String(value));
}

async function onRiskUpdate(value: number): Promise<void> {
  const taskKey = key();
  if (!taskKey) return;
  await updateTaskField(taskKey, "risk", String(value));
}

async function onTagsUpdate(tags: string[]): Promise<void> {
  const taskKey = key();
  if (!taskKey) return;
  const ok = await setTaskTags(taskKey, tags);
  if (ok) {
    taskTags.value = tags;
    cacheTags(taskKey, tags);
  }
}
</script>

<template>
  <div class="inspector">
    <div v-if="!task" class="inspector__empty">
      <EmptyState icon="fa-pen-to-square" title="No Selection" description="Select a task to view and edit its properties" />
    </div>
    <div v-else class="inspector__content">
      <TitleEditor :disabled="!canEditField" @update:title="onTitleUpdate" />

      <div v-if="!canEditField" class="inspector__notice">
        <i class="fas fa-triangle-exclamation"></i>
        Editing is disabled: the backend has no <code>update_task_field</code> command yet.
      </div>

      <div class="inspector__fields">
        <StatusEditor
          :value="task.status"
          :disabled="!canEditField"
          @update:value="onStatusUpdate"
        />

        <PriorityEditor
          :value="task.priority"
          :disabled="!canEditField"
          @update:value="onPriorityUpdate"
        />

        <div class="inspector__row">
          <DateEditor
            :value="task.start_date"
            label="Start Date"
            :disabled="!canEditField"
            @update:value="onStartDateUpdate"
          />
          <DateEditor
            :value="task.due_date"
            label="Due Date"
            :disabled="!canEditField"
            @update:value="onDueDateUpdate"
          />
        </div>

        <TagsEditor
          :tags="taskTags"
          label="Tags"
          :disabled="!canEditTags"
          @update:tags="onTagsUpdate"
        />

        <ParticipantsEditor :task-key="task.task_key" :document-id="task.document_id" />
        <DependencyEditor :task-key="task.task_key" :document-id="task.document_id" />
        <ProgressLinksEditor :task-key="task.task_key" :document-id="task.document_id" />
        <AttachmentListEditor :task-key="task.task_key" :document-id="task.document_id" />

        <MoreProperties
          :task-key="task.task_key"
          :document-id="task.document_id"
          :percent-done="task.percent_done"
          :risk="task.risk"
          :completed-date="task.completed_date"
          :disabled="!canEditField"
          @update:percent-done="onPercentDoneUpdate"
          @update:risk="onRiskUpdate"
          @update:completed-date="onCompletedDateUpdate"
        />
      </div>
    </div>
  </div>
</template>

<style scoped>
.inspector {
  display: flex;
  flex-direction: column;
  height: 100%;
}
.inspector__empty {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
}
.inspector__content {
  padding: var(--space-4);
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}
.inspector__fields {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}
.inspector__row {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: var(--space-3);
}
.inspector__notice {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-xs);
  color: var(--color-warning);
  background: var(--color-bg-tertiary);
  border-radius: var(--radius-md);
  padding: var(--space-2);
}
.inspector__notice code {
  font-family: var(--font-mono);
}
</style>
