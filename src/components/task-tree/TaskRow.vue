<script setup lang="ts">
import { computed, ref } from "vue";
import type { TaskRow as TaskRowData } from "../../stores/task-store";
import {
  childCount, getTask, isTaskBlocked, toggleGroup, toggleTaskStatus,
} from "../../stores/task-store";
import { selectedTaskKey, toggleExpand, selectTask, showToast } from "../../stores/app-state";
import { toggleMultiSelect, clearMultiSelect, isMultiSelected } from "../../stores/filter-state";
import { classifyDependency, outgoingDependencies } from "../../stores/relation-store";
import { isCommandAvailable } from "../../stores/capability-store";
import { Commands } from "../../ipc/commands";

const props = defineProps<{
  row: TaskRowData;
}>();

const busy = ref(false);

const isGroup = computed(() => props.row.kind === "group");
const taskKey = computed(() => props.row.task.task_key);
const children = computed(() => childCount(taskKey.value));
const blocked = computed(() => !isGroup.value && isTaskBlocked(taskKey.value));
const checkboxDisabled = computed(() =>
  isGroup.value || busy.value || !isCommandAvailable(Commands.UPDATE_TASK_FIELD),
);

const blockReasons = computed<string[]>(() => {
  if (!blocked.value) return [];
  const reasons: string[] = [];
  for (const dep of outgoingDependencies(taskKey.value)) {
    const state = classifyDependency(dep, (key) => getTask(key) !== undefined);
    const target = getTask(dep.depends_on_key);
    const label = target?.title ?? dep.depends_on_key;
    if (state === "circular") reasons.push(`circular reference: ${label}`);
    else if (state === "unresolved") reasons.push(`unresolved reference: ${dep.depends_on_key}`);
    else if (target && target.status !== "Completed" && target.status !== "Cancelled") {
      reasons.push(`${label} (${target.status || "Not Started"})`);
    }
  }
  return reasons;
});

const blockedTitle = computed(() =>
  blockReasons.value.length > 0
    ? `Blocked by ${blockReasons.value.length}: ${blockReasons.value.join("; ")}`
    : "Blocked",
);

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

const isSelected = () => selectedTaskKey.value === taskKey.value;

function handleClick(e: MouseEvent) {
  if (isGroup.value) {
    toggleGroup(props.row.groupLabel);
    return;
  }
  if (e.ctrlKey || e.metaKey || e.shiftKey) {
    toggleMultiSelect(taskKey.value);
  } else {
    clearMultiSelect();
  }
  selectTask(taskKey.value);
}

function handleToggle(e: Event) {
  e.stopPropagation();
  if (isGroup.value) {
    toggleGroup(props.row.groupLabel);
    return;
  }
  toggleExpand(taskKey.value);
}

async function handleCheckbox(e: Event) {
  e.stopPropagation();
  if (checkboxDisabled.value) return;
  const checked = (e.target as HTMLInputElement).checked;
  busy.value = true;
  try {
    const ok = await toggleTaskStatus(taskKey.value);
    if (!ok && !isCommandAvailable(Commands.UPDATE_TASK_FIELD)) {
      showToast("Task editing is unavailable: the backend has no update_task_field command", "warning");
    }
    // Restore the visual state when the mutation was rejected.
    if (!ok) (e.target as HTMLInputElement).checked = !checked;
  } finally {
    busy.value = false;
  }
}

function formatDate(d: string | null): string {
  if (!d) return "";
  // Simple YYYY-MM-DD display
  return d;
}
</script>

<template>
  <!-- Group header row (participant grouping view mode) -->
  <div
    v-if="isGroup"
    class="task-row task-row--group"
    :data-task-key="row.task.task_key"
    @click="handleClick($event)"
  >
    <button class="task-row__toggle" @click="handleToggle">
      <i :class="row.isExpanded ? 'fas fa-chevron-down' : 'fas fa-chevron-right'"></i>
    </button>
    <i class="fas fa-users task-row__group-icon"></i>
    <span class="task-row__group-label">{{ row.groupLabel }}</span>
    <span class="task-row__child-count">{{ row.groupSize }}</span>
  </div>

  <!-- Task row -->
  <div
    v-else
    class="task-row"
    :class="{
      'task-row--selected': isSelected(),
      'task-row--multi': isMultiSelected(row.task.task_key),
      'task-row--blocked': blocked,
    }"
    :data-task-key="row.task.task_key"
    :style="{ paddingLeft: `${row.depth * 20 + 8}px` }"
    role="treeitem"
    :aria-selected="isSelected()"
    :aria-expanded="row.hasChildren ? row.isExpanded : undefined"
    @click="handleClick($event)"
  >
    <!-- Expand/collapse toggle -->
    <button
      v-if="row.hasChildren"
      class="task-row__toggle"
      :aria-label="row.isExpanded ? 'Collapse' : 'Expand'"
      @click="handleToggle"
    >
      <i :class="row.isExpanded ? 'fas fa-chevron-down' : 'fas fa-chevron-right'"></i>
    </button>
    <span v-else class="task-row__toggle-placeholder"></span>

    <!-- Completion checkbox (Phase 0.1: enabled and wired to update_task_field) -->
    <label class="task-row__checkbox" @click.stop>
      <input
        type="checkbox"
        :checked="row.task.status === 'Completed'"
        :disabled="checkboxDisabled"
        :title="checkboxDisabled ? 'Task editing unavailable' : 'Toggle completion'"
        @change="handleCheckbox"
      />
    </label>

    <!-- Priority indicator -->
    <span
      class="task-row__priority"
      :class="priorityClass(row.task.priority)"
      :title="`Priority: ${priorityLabel(row.task.priority)}`"
    ></span>

    <!-- Blocked indicator (M6 dependencies) -->
    <i
      v-if="blocked"
      class="task-row__blocked fas fa-link-slash"
      :title="blockedTitle"
    ></i>

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

    <!-- Child count badge (Phase 0.1: real child count) -->
    <span v-if="children > 0" class="task-row__child-count" :title="`${children} subtask(s)`">
      {{ children }}
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
.task-row--multi {
  box-shadow: inset 2px 0 0 var(--color-accent);
}
.task-row--blocked .task-row__title {
  color: var(--color-text-secondary);
  border-bottom: 1px dashed var(--color-warning);
}

.task-row--group {
  background: var(--color-bg-tertiary);
  border-bottom: 1px solid var(--color-border);
  font-weight: 600;
  position: sticky;
  top: 0;
  z-index: 1;
}
.task-row__group-icon {
  font-size: var(--text-xs);
  color: var(--color-text-muted);
}
.task-row__group-label {
  flex: 1;
  font-size: var(--text-sm);
  color: var(--color-text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
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
  display: flex;
}
.task-row__checkbox input {
  width: 14px;
  height: 14px;
  cursor: pointer;
  accent-color: var(--color-accent);
}
.task-row__checkbox input:disabled {
  cursor: not-allowed;
  opacity: 0.5;
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

.task-row__blocked {
  font-size: var(--text-xs);
  color: var(--color-warning);
  flex-shrink: 0;
}

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
