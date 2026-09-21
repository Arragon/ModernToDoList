<script setup lang="ts">
import { watch } from "vue";
import { workspace, documents } from "../../stores/app-state";
import { loadTasks, expandRootTasks } from "../../stores/task-store";
import TaskTree from "../task-tree/TaskTree.vue";
import TaskFilter from "../task-tree/TaskFilter.vue";
import EmptyState from "../common/EmptyState.vue";

const hasWorkspace = () => workspace.value !== null;

// Auto-load tasks when documents change
watch(documents, async (docs) => {
  if (docs.length > 0) {
    await loadTasks();
    expandRootTasks();
  }
}, { immediate: true });
</script>

<template>
  <div class="task-tree-panel">
    <div class="task-tree-panel__header">
      <h3>Tasks</h3>
      <button class="btn" title="Add Task" @click="() => {}">
        <i class="fas fa-plus"></i>
      </button>
    </div>
    <div class="task-tree-panel__content">
      <TaskFilter v-if="hasWorkspace() && documents.length > 0" />
      <EmptyState v-if="!hasWorkspace()" icon="fa-list-check" title="No Workspace" description="Open a workspace to view and manage tasks" />
      <EmptyState v-else-if="documents.length === 0" icon="fa-file-circle-xmark" title="No Documents" description="Scan the workspace to discover task documents" />
      <TaskTree v-else />
    </div>
  </div>
</template>

<style scoped>
.task-tree-panel {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-width: 300px;
  background: var(--color-bg-primary);
  border-right: 1px solid var(--color-border);
  overflow: hidden;
}
.task-tree-panel__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-2) var(--space-4);
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
  height: var(--toolbar-height);
}
.task-tree-panel__header h3 { font-size: var(--text-sm); font-weight: 600; }
.task-tree-panel__content { flex: 1; overflow-y: auto; display: flex; flex-direction: column; }
</style>
