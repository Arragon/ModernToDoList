<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onBeforeUpdate, onUpdated, ref, watch } from "vue";
import {
  clearPendingScroll, pendingScrollKey, tasksLoading, visibleRowIndex, visibleRows,
} from "../../stores/task-store";
import { settings, virtualizationActive } from "../../stores/settings-store";
import { beginRender, endRender } from "../../app/perf";
import type { RenderTimer } from "../../app/perf";
import TaskRow from "./TaskRow.vue";
import VirtualList from "./VirtualList.vue";
import LoadingState from "../common/LoadingState.vue";
import EmptyState from "../common/EmptyState.vue";

const rows = computed(() => visibleRows.value);

/** RD-M10-022 gate: mode "on"/"off" forces, "auto" enables above the threshold. */
const virtualized = computed(() => virtualizationActive(rows.value.length));

const listRef = ref<InstanceType<typeof VirtualList> | null>(null);
const plainListRef = ref<HTMLElement | null>(null);

let fullRenderTimer: RenderTimer | null = null;

onBeforeUpdate(() => {
  if (!virtualized.value) fullRenderTimer = beginRender("task-tree:render-full");
});

onUpdated(() => {
  if (fullRenderTimer) {
    endRender(fullRenderTimer, {
      rowCount: rows.value.length,
      renderedCount: rows.value.length,
      virtualized: false,
    });
    fullRenderTimer = null;
  }
});

onBeforeUnmount(() => {
  fullRenderTimer = null;
});

/** Scrolls the tree so the requested task is visible (search / palette jump). */
async function scrollToKey(taskKey: string): Promise<void> {
  await nextTick();
  const index = visibleRowIndex(taskKey);
  if (index < 0) {
    clearPendingScroll();
    return;
  }
  if (virtualized.value) {
    listRef.value?.scrollToIndex(index, "center");
  } else {
    const el = plainListRef.value?.querySelector<HTMLElement>(
      `[data-task-key="${cssEscape(taskKey)}"]`,
    );
    el?.scrollIntoView({ block: "center", behavior: "auto" });
  }
  clearPendingScroll();
}

function cssEscape(value: string): string {
  return typeof CSS !== "undefined" && typeof CSS.escape === "function"
    ? CSS.escape(value)
    : value.replace(/["\\]/g, "\\$&");
}

watch(pendingScrollKey, (key) => {
  if (!key) return;
  void scrollToKey(key);
});

// When the row set changes, a pending jump may only become possible now.
watch(() => rows.value.length, () => {
  const key = pendingScrollKey.value;
  if (key) void scrollToKey(key);
});

// Switching virtualization on/off invalidates measured heights.
watch(virtualized, () => {
  listRef.value?.remeasure();
});
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

    <VirtualList
      v-else-if="virtualized"
      ref="listRef"
      class="task-tree__scroller"
      :item-count="rows.length"
      :item-height="settings.virtualization.rowHeight"
      :overscan="settings.virtualization.overscan"
      :dynamic="settings.virtualization.dynamicHeights"
      aria-label="Task tree"
    >
      <template #item="{ index }">
        <TaskRow :row="rows[index]" />
      </template>
    </VirtualList>

    <div v-else ref="plainListRef" class="task-tree__scroller task-tree__list" role="tree">
      <TaskRow v-for="row in rows" :key="row.task.task_key" :row="row" />
    </div>

    <div class="task-tree__statusbar">
      <span>{{ rows.length }} row(s)</span>
      <span :class="['task-tree__virt', virtualized ? 'task-tree__virt--on' : 'task-tree__virt--off']">
        <i :class="virtualized ? 'fas fa-bolt' : 'fas fa-list'"></i>
        virtualization {{ virtualized ? 'on' : 'off' }} ({{ settings.virtualization.mode }})
      </span>
    </div>
  </div>
</template>

<style scoped>
.task-tree {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-height: 0;
  overflow: hidden;
}
.task-tree__scroller {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
}
.task-tree__list {
  padding: var(--space-1) 0;
}
.task-tree__statusbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-2);
  height: var(--statusbar-height);
  padding: 0 var(--space-3);
  border-top: 1px solid var(--color-border);
  background: var(--color-bg-secondary);
  font-size: var(--text-xs);
  color: var(--color-text-muted);
  flex-shrink: 0;
}
.task-tree__virt {
  display: inline-flex;
  align-items: center;
  gap: var(--space-1);
}
.task-tree__virt--on { color: var(--color-success); }
.task-tree__virt--off { color: var(--color-text-muted); }
</style>
