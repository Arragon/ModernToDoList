<script setup lang="ts">
import { visibleRows, tasksLoading } from "../../stores/task-store";
import TaskRow from "./TaskRow.vue";
import LoadingState from "../common/LoadingState.vue";
import EmptyState from "../common/EmptyState.vue";

const rows = visibleRows;
</script>

<template>
  <div class="task-tree">
    <LoadingState v-if="tasksLoading" message="Loading tasks..." />
    <EmptyState
      v-else-if="rows.length === 0"
      icon="fa-list"
      title="No Tasks"
      description="No tasks found in the index. Scan your workspace to populate."
    />
    <div v-else class="task-tree__list" role="tree">
      <TaskRow
        v-for="row in rows"
        :key="row.task.task_key"
        :row="row"
      />
    </div>
  </div>
</template>

<style scoped>
.task-tree {
  flex: 1;
  overflow-y: auto;
}
.task-tree__list {
  padding: var(--space-1) 0;
}
</style>
