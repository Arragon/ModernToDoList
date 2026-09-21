<script setup lang="ts">
import type { TaskRow as TaskRowData } from "../../stores/task-store";
import { selectedTaskKey, toggleExpand, selectTask } from "../../stores/app-state";
import { toggleMultiSelect, clearMultiSelect, isMultiSelected } from "../../stores/filter-state";

const props = defineProps<{
  row: TaskRowData;
}>();

const priorityLabel = (p: number) => {
  if (p >= 4) return "Very High";
  if (p === 3) return "High";
  if (p === 2) return "Medium";
  if (p === 1) return "Low";
  return "None";
};

const priorityClass = (p: number) => {
  if (p >= 4) return "priority--very-high";
  if (p === 3) return "priority--high";
  if (p === 2) return "priority--medium";
  if (p === 1) return "priority--low";
  return "priority--none";
};

const statusIcon = (s: string) => {
  if (s === "Completed") return "fa-circle-check";
  if (s === "In Progress") return "fa-spinner";
  if (s === "Blocked") return "fa-ban";
  return "fa-circle";
};

const isSelected = () => selectedTaskKey.value === props.row.task.task_key;

function handleClick(e: MouseEvent) {
  if (e.ctrlKey || e.metaKey) {
    toggleMultiSelect(props.row.task.task_key);
  } else if (e.shiftKey) {
    toggleMultiSelect(props.row.task.task_key);
  } else {
    clearMultiSelect();
  }
  selectTask(props.row.task.task_key);
}

function handleToggle(e: Event) {
  e.stopPropagation();
  toggleExpand(props.row.task.task_key);
}

function formatDate(d: string | null): string {
  if (!d) return "";
  // Simple YYYY-MM-DD display
  return d;
}
</script>

<template>
  <div
    class="task-row"
    :class="{ 'task-row--selected': isSelected(), 'task-row--multi': isMultiSelected(row.task.task_key) }"
    :style="{ paddingLeft: `${row.depth * 20 + 8}px` }"
    @click="handleClick($event)"
  >
    <!-- Expand/collapse toggle -->
    <button
      v-if="row.hasChildren"
      class="task-row__toggle"
      @click="handleToggle"
    >
      <i :class="row.isExpanded ? 'fas fa-chevron-down' : 'fas fa-chevron-right'"></i>
    </button>
    <span v-else class="task-row__toggle-placeholder"></span>

    <!-- Completion checkbox -->
    <label class="task-row__checkbox" @click.stop>
      <input
        type="checkbox"
        :checked="row.task.status === 'Completed'"
        :disabled="true"
      />
    </label>

    <!-- Priority indicator -->
    <span
      class="task-row__priority"
      :class="priorityClass(row.task.priority)"
      :title="`Priority: ${priorityLabel(row.task.priority)}`"
    ></span>

    <!-- Title -->
    <span class="task-row__title">{{ row.task.title || '(Untitled)' }}</span>

    <!-- Status icon -->
    <i
      v-if="row.task.status && row.task.status !== 'Not Started'"
      class="task-row__status fas"
      :class="statusIcon(row.task.status)"
      :title="row.task.status"
    ></i>

    <!-- Percent done -->
    <span v-if="row.task.percent_done > 0" class="task-row__percent">
      {{ row.task.percent_done }}%
    </span>

    <!-- Due date -->
    <span v-if="row.task.due_date" class="task-row__date" :title="'Due: ' + formatDate(row.task.due_date)">
      <i class="fas fa-calendar-day"></i>
      {{ formatDate(row.task.due_date) }}
    </span>

    <!-- Child count badge -->
    <span v-if="row.hasChildren" class="task-row__child-count">
      {{ row.task.task_key }}
    </span>
  </div>
</template>

<style scoped>
.task-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-1) var(--space-2);
  cursor: pointer;
  user-select: none;
  min-height: 32px;
  border-bottom: 1px solid transparent;
  transition: background var(--transition-fast);
}
.task-row:hover {
  background: var(--color-bg-hover);
}
.task-row--selected {
  background: var(--color-bg-selected);
}
.task-row--selected:hover {
  background: var(--color-bg-active);
}

.task-row__toggle {
  background: none;
  border: none;
  padding: 2px;
  cursor: pointer;
  color: var(--color-text-muted);
  width: 20px;
  height: 20px;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  font-size: var(--text-xs);
}
.task-row__toggle:hover {
  color: var(--color-text-primary);
}
.task-row__toggle-placeholder {
  width: 20px;
  flex-shrink: 0;
}

.task-row__checkbox {
  flex-shrink: 0;
  cursor: pointer;
}
.task-row__checkbox input {
  width: 14px;
  height: 14px;
  cursor: pointer;
}

.task-row__priority {
  width: 8px;
  height: 8px;
  border-radius: var(--radius-full);
  flex-shrink: 0;
}
.priority--none { background: var(--priority-none); }
.priority--low { background: var(--priority-low); }
.priority--medium { background: var(--priority-medium); }
.priority--high { background: var(--priority-high); }
.priority--very-high { background: var(--priority-very-high); }

.task-row__title {
  flex: 1;
  font-size: var(--text-sm);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.task-row__status {
  font-size: var(--text-xs);
  color: var(--color-text-muted);
  flex-shrink: 0;
}

.task-row__percent {
  font-size: var(--text-xs);
  color: var(--color-text-muted);
  flex-shrink: 0;
}

.task-row__date {
  font-size: var(--text-xs);
  color: var(--color-text-secondary);
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 2px;
}
.task-row__date i {
  font-size: 10px;
}

.task-row__child-count {
  font-size: var(--text-xs);
  color: var(--color-text-muted);
  background: var(--color-bg-tertiary);
  padding: 0 var(--space-1);
  border-radius: var(--radius-sm);
  flex-shrink: 0;
}
</style>
