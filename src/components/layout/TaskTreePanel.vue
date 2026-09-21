<script setup lang="ts">
import { computed, watch } from "vue";
import { documents, selectedTaskKey, workspace } from "../../stores/app-state";
import { allTasks, expandRootTasks, loadTasks } from "../../stores/task-store";
import { filterState, multiSelectedKeys, setGroupMode } from "../../stores/filter-state";
import { loadDependencies, loadParticipants } from "../../stores/relation-store";
import { openGlobalSearch, globalSearchOpen } from "../../stores/ui-store";
import { executeCommand } from "../../app/commands";
import { settings } from "../../stores/settings-store";
import TaskTree from "../task-tree/TaskTree.vue";
import TaskFilter from "../task-tree/TaskFilter.vue";
import GlobalSearch from "../search/GlobalSearch.vue";
import QuickAdd from "../search/QuickAdd.vue";
import EmptyState from "../common/EmptyState.vue";

const hasWorkspace = computed(() => workspace.value !== null);
const hasDocuments = computed(() => documents.value.length > 0);
const selectionCount = computed(() => multiSelectedKeys.value.size);
const grouped = computed(() => filterState.value.groupBy === "participant");

// Auto-load tasks when documents change
watch(documents, async (docs) => {
  if (docs.length > 0) {
    await loadTasks();
    expandRootTasks();
    await Promise.all([loadParticipants(true), loadDependencies(true)]);
  }
}, { immediate: true });

async function toggleGrouping(): Promise<void> {
  const next = grouped.value ? "none" : "participant";
  if (next === "participant") await loadParticipants(true);
  setGroupMode(next);
}

function addTask(): void {
  executeCommand(selectedTaskKey.value ? "task.addChild" : "task.addRoot");
}

function toggleVirtualization(): void {
  const order = ["auto", "on", "off"] as const;
  const index = order.indexOf(settings.virtualization.mode);
  settings.virtualization.mode = order[(index + 1) % order.length];
}
</script>

<template>
  <div class="task-tree-panel">
    <div class="task-tree-panel__header">
      <h3>
        Tasks
        <span v-if="selectionCount > 0" class="task-tree-panel__selection">
          {{ selectionCount }} selected
        </span>
      </h3>
      <div class="task-tree-panel__tools">
        <button
          class="task-tree-panel__tool"
          :class="{ 'task-tree-panel__tool--on': grouped }"
          :disabled="!hasDocuments"
          title="Group by participant (RD-M6-008)"
          @click="toggleGrouping"
        >
          <i class="fas fa-users"></i>
        </button>
        <button
          class="task-tree-panel__tool"
          :title="`Tree virtualization: ${settings.virtualization.mode} (click to cycle)`"
          @click="toggleVirtualization"
        >
          <i :class="settings.virtualization.mode === 'off' ? 'fas fa-list' : 'fas fa-bolt'"></i>
        </button>
        <button
          class="task-tree-panel__tool"
          :class="{ 'task-tree-panel__tool--on': globalSearchOpen }"
          :disabled="!hasDocuments"
          title="Global Search (Ctrl+F)"
          @click="openGlobalSearch()"
        >
          <i class="fas fa-magnifying-glass"></i>
        </button>
        <button
          class="task-tree-panel__tool"
          :disabled="!hasDocuments"
          title="Add task (Ctrl+N / Ctrl+Shift+N)"
          @click="addTask"
        >
          <i class="fas fa-plus"></i>
        </button>
      </div>
    </div>

    <div class="task-tree-panel__content">
      <GlobalSearch />
      <TaskFilter v-if="hasWorkspace && hasDocuments" />
      <EmptyState
        v-if="!hasWorkspace"
        icon="fa-list-check"
        title="No Workspace"
        description="Open a workspace to view and manage tasks"
      />
      <EmptyState
        v-else-if="!hasDocuments"
        icon="fa-file-circle-xmark"
        title="No Documents"
        description="Scan the workspace to discover task documents"
      />
      <TaskTree v-else />
    </div>

    <QuickAdd v-if="hasDocuments && settings.quickAddEnabled" />

    <div v-if="hasDocuments" class="task-tree-panel__status">
      <span>{{ allTasks.length }} task(s) indexed</span>
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
  gap: var(--space-2);
  padding: var(--space-2) var(--space-4);
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
  height: var(--toolbar-height);
}
.task-tree-panel__header h3 {
  font-size: var(--text-sm);
  font-weight: 600;
  display: flex;
  align-items: center;
  gap: var(--space-2);
}
.task-tree-panel__selection {
  font-size: var(--text-xs);
  font-weight: 400;
  color: var(--color-accent);
  background: var(--color-accent-light);
  border-radius: var(--radius-full);
  padding: 0 var(--space-2);
}
.task-tree-panel__tools {
  display: flex;
  align-items: center;
  gap: 2px;
}
.task-tree-panel__tool {
  background: none;
  border: none;
  cursor: pointer;
  color: var(--color-text-muted);
  padding: var(--space-1);
  border-radius: var(--radius-sm);
  font-size: var(--text-sm);
}
.task-tree-panel__tool:hover:not(:disabled) {
  color: var(--color-text-primary);
  background: var(--color-bg-hover);
}
.task-tree-panel__tool--on {
  color: var(--color-accent);
  background: var(--color-accent-light);
}
.task-tree-panel__tool:disabled {
  color: var(--color-text-disabled);
  cursor: not-allowed;
}
.task-tree-panel__content {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.task-tree-panel__status {
  flex-shrink: 0;
  padding: 0 var(--space-3);
  font-size: var(--text-xs);
  color: var(--color-text-muted);
}
</style>
