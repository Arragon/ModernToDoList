<script setup lang="ts">
/**
 * Task filter bar.
 *
 * Title keyword, status, due-date window and the M6 participant condition
 * (RD-M6-007). Participant suggestions come from the workspace index.
 */
import { computed, onMounted, ref } from "vue";
import {
  clearFilters, clearParticipantFilter, filterState, isFilterActive,
  toggleParticipantFilter,
} from "../../stores/filter-state";
import { loadParticipants, participantSuggestions } from "../../stores/relation-store";
import { fuzzyRank } from "../../app/fuzzy";
import type { FilterState } from "../../stores/filter-state";

type DueRange = FilterState["dueDateRange"];

const DUE_RANGES: DueRange[] = ["all", "overdue", "today", "this-week", "no-date"];

const dueDateOptions: Array<{ value: DueRange; label: string }> = [
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

const participantPickerOpen = ref(false);
const participantQuery = ref("");

const selectedParticipants = computed(() => filterState.value.participants);
const suggestions = computed<string[]>(() => {
  const query = participantQuery.value.trim();
  const pool = participantSuggestions.value;
  if (!query) return pool.slice(0, 12);
  return fuzzyRank(pool, query, (name) => name, undefined, 12).map((entry) => entry.item);
});

onMounted(() => {
  void loadParticipants();
});

function onStatusChange(e: Event) {
  const val = (e.target as HTMLSelectElement).value;
  filterState.value.statusFilter = val || null;
}

function onDueChange(e: Event) {
  const value = (e.target as HTMLSelectElement).value as DueRange;
  filterState.value.dueDateRange = DUE_RANGES.includes(value) ? value : "all";
}

function onParticipantModeChange(e: Event) {
  const value = (e.target as HTMLSelectElement).value;
  filterState.value.participantMode = value === "all" ? "all" : "any";
}

function pick(name: string): void {
  toggleParticipantFilter(name);
  participantQuery.value = "";
}

function clearPeople(): void {
  clearParticipantFilter();
  participantQuery.value = "";
}

function onClear(): void {
  clearFilters();
  participantQuery.value = "";
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
        class="task-filter__people"
        :class="{ 'task-filter__people--on': selectedParticipants.length > 0 }"
        title="Filter by participant"
        @click="participantPickerOpen = !participantPickerOpen"
      >
        <i class="fas fa-users"></i>
        <span v-if="selectedParticipants.length > 0">{{ selectedParticipants.length }}</span>
      </button>
      <button
        v-if="isFilterActive"
        class="task-filter__clear"
        title="Clear filters"
        @click="onClear"
      >
        <i class="fas fa-xmark"></i>
      </button>
    </div>

    <div v-if="participantPickerOpen" class="task-filter__participants">
      <div class="task-filter__participant-head">
        <input
          v-model="participantQuery"
          class="task-filter__search"
          type="text"
          placeholder="Search participants…"
        />
        <select
          class="task-filter__select"
          :value="filterState.participantMode"
          @change="onParticipantModeChange"
        >
          <option value="any">Any of</option>
          <option value="all">All of</option>
        </select>
        <button
          v-if="selectedParticipants.length > 0"
          class="task-filter__clear"
          title="Clear participant filter"
          @click="clearPeople"
        >
          <i class="fas fa-eraser"></i>
        </button>
      </div>

      <div v-if="selectedParticipants.length > 0" class="task-filter__chips">
        <span v-for="name in selectedParticipants" :key="`sel-${name}`" class="chip">
          <i class="fas fa-user"></i>
          {{ name }}
          <button class="task-filter__chip-x" @click="toggleParticipantFilter(name)">
            <i class="fas fa-xmark"></i>
          </button>
        </span>
      </div>

      <div class="task-filter__suggest">
        <button
          v-for="name in suggestions"
          :key="`sug-${name}`"
          class="task-filter__suggest-chip"
          :class="{ 'task-filter__suggest-chip--on': selectedParticipants.includes(name) }"
          @click="pick(name)"
        >
          {{ name }}
        </button>
        <span v-if="suggestions.length === 0" class="task-filter__hint">
          No participant names in the workspace index yet.
        </span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.task-filter {
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
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
.task-filter__people {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  background: none;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  cursor: pointer;
  color: var(--color-text-muted);
  padding: var(--space-1) var(--space-2);
  font-size: var(--text-xs);
}
.task-filter__people:hover { color: var(--color-text-primary); }
.task-filter__people--on {
  color: var(--color-accent);
  border-color: var(--color-accent);
  background: var(--color-accent-light);
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
.task-filter__participants {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  border-top: 1px dashed var(--color-border);
  padding-top: var(--space-2);
}
.task-filter__participant-head {
  display: flex;
  gap: var(--space-2);
  align-items: center;
}
.task-filter__chips,
.task-filter__suggest {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-1);
}
.task-filter__chip-x {
  background: none;
  border: none;
  cursor: pointer;
  color: var(--color-text-muted);
  font-size: var(--text-xs);
  padding: 0;
  line-height: 1;
}
.task-filter__chip-x:hover { color: var(--color-error); }
.task-filter__suggest-chip {
  border: 1px dashed var(--color-border-hover);
  background: var(--color-bg-primary);
  color: var(--color-text-secondary);
  border-radius: var(--radius-full);
  font-size: var(--text-xs);
  padding: 1px var(--space-2);
  cursor: pointer;
}
.task-filter__suggest-chip:hover {
  border-color: var(--color-accent);
  color: var(--color-accent);
}
.task-filter__suggest-chip--on {
  border-style: solid;
  border-color: var(--color-accent);
  background: var(--color-accent-light);
  color: var(--color-accent);
}
.task-filter__hint {
  font-size: var(--text-xs);
  color: var(--color-text-muted);
  font-style: italic;
}
</style>
