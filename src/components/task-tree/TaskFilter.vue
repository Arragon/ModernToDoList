<script setup lang="ts">
import { filterState, isFilterActive, clearFilters } from "../../stores/filter-state";

const dueDateOptions = [
  { value: "all", label: "All" },
  { value: "overdue", label: "Overdue" },
  { value: "today", label: "Today" },
  { value: "this-week", label: "This Week" },
  { value: "no-date", label: "No Due Date" },
];

const statusOptions = [
  { value: "", label: "All" },
  { value: "Not Started", label: "Not Started" },
  { value: "In Progress", label: "In Progress" },
  { value: "Completed", label: "Completed" },
  { value: "Blocked", label: "Blocked" },
];

function onStatusChange(e: Event) {
  const val = (e.target as HTMLSelectElement).value;
  filterState.value.statusFilter = val || null;
}

function onDueChange(e: Event) {
  filterState.value.dueDateRange = (e.target as HTMLSelectElement).value as typeof filterState.value.dueDateRange;
}
</script>

<template>
  <div class="task-filter">
    <div class="task-filter__row">
      <input
        class="task-filter__search"
        type="text"
        v-model="filterState.titleKeyword"
        placeholder="Filter by title..."
      />
      <select class="task-filter__select" :value="filterState.statusFilter ?? ''" @change="onStatusChange">
        <option v-for="opt in statusOptions" :key="opt.value" :value="opt.value">{{ opt.label }}</option>
      </select>
      <select class="task-filter__select" :value="filterState.dueDateRange" @change="onDueChange">
        <option v-for="opt in dueDateOptions" :key="opt.value" :value="opt.value">{{ opt.label }}</option>
      </select>
      <button
        v-if="isFilterActive"
        class="task-filter__clear"
        title="Clear filters"
        @click="clearFilters"
      >
        <i class="fas fa-xmark"></i>
      </button>
    </div>
  </div>
</template>

<style scoped>
.task-filter {
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
}
.task-filter__row {
  display: flex;
  gap: var(--space-2);
  align-items: center;
}
.task-filter__search {
  flex: 1;
  padding: var(--space-1) var(--space-2);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  font-size: var(--text-xs);
  background: var(--color-bg-primary);
  color: var(--color-text-primary);
  outline: none;
  min-width: 0;
}
.task-filter__search:focus {
  border-color: var(--color-border-focus);
}
.task-filter__select {
  padding: var(--space-1) var(--space-2);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  font-size: var(--text-xs);
  background: var(--color-bg-primary);
  color: var(--color-text-primary);
  outline: none;
  cursor: pointer;
}
.task-filter__clear {
  background: none;
  border: none;
  cursor: pointer;
  color: var(--color-text-muted);
  padding: var(--space-1);
  font-size: var(--text-sm);
}
.task-filter__clear:hover {
  color: var(--color-error);
}
</style>
