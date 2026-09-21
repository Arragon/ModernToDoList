<script setup lang="ts">
import { ref, watch } from "vue";
import { selectedTask } from "../../stores/task-store";
import * as ipc from "../../ipc/client";
import TitleEditor from "./TitleEditor.vue";
import StatusEditor from "./StatusEditor.vue";
import PriorityEditor from "./PriorityEditor.vue";
import DateEditor from "./DateEditor.vue";
import TagsEditor from "./TagsEditor.vue";
import MoreProperties from "./MoreProperties.vue";
import EmptyState from "../common/EmptyState.vue";

const taskTags = ref<string[]>([]);

watch(selectedTask, async (task) => {
  if (task) {
    try {
      taskTags.value = await ipc.getTaskTags(task.task_key);
    } catch {
      taskTags.value = [];
    }
  } else {
    taskTags.value = [];
  }
}, { immediate: true });
</script>

<template>
  <div class="inspector">
    <div v-if="!selectedTask" class="inspector__empty">
      <EmptyState icon="fa-pen-to-square" title="No Selection" description="Select a task to view and edit its properties" />
    </div>
    <div v-else class="inspector__content">
      <TitleEditor />

      <div class="inspector__fields">
        <StatusEditor
          :value="selectedTask.status"
          @update:value="() => {}"
        />

        <PriorityEditor
          :value="selectedTask.priority"
          @update:value="() => {}"
        />

        <div class="inspector__row">
          <DateEditor
            :value="selectedTask.start_date"
            label="Start Date"
            @update:value="() => {}"
          />
          <DateEditor
            :value="selectedTask.due_date"
            label="Due Date"
            @update:value="() => {}"
          />
        </div>

        <TagsEditor
          :tags="taskTags"
          label="Tags"
          @update:tags="() => {}"
        />

        <MoreProperties
          :task-key="selectedTask.task_key"
          :document-id="selectedTask.document_id"
          :percent-done="selectedTask.percent_done"
          :risk="selectedTask.risk"
          :completed-date="selectedTask.completed_date"
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
</style>
