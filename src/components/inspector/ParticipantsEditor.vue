<script setup lang="ts">
/**
 * Participants editor (RD-M6-004~006).
 *
 *  - chips with a per-chip remove action
 *  - add picker with suggestions sourced from the workspace index
 *  - bulk multi-select assignment for the current tree selection
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import type { ParticipantDto, TaskSummary } from "../../ipc/types";
import {
  addParticipant, bulkAssignParticipants, loadParticipants, loadTaskParticipants,
  participantSuggestions, participantsAvailable, participantsFor, removeParticipant,
} from "../../stores/relation-store";
import type { TaskIdentity } from "../../stores/relation-store";
import { multiSelectedKeys } from "../../stores/filter-state";
import { getTask } from "../../stores/task-store";
import { showToast } from "../../stores/app-state";
import { fuzzyRank } from "../../app/fuzzy";

const props = defineProps<{
  taskKey: string;
  documentId: string;
}>();

const identity = computed<TaskIdentity>(() => ({
  task_key: props.taskKey,
  document_id: props.documentId,
}));

const draft = ref("");
const busy = ref(false);
const pickerOpen = ref(false);
const bulkOpen = ref(false);
const bulkSelection = ref<Set<string>>(new Set());

const rows = computed<ParticipantDto[]>(() => participantsFor(props.taskKey));
const available = computed(() => participantsAvailable.value);
const multiCount = computed(() => multiSelectedKeys.value.size);

const suggestions = computed<string[]>(() => {
  const existing = new Set(rows.value.map((r) => r.display_name.toLowerCase()));
  const candidates = participantSuggestions.value.filter((n) => !existing.has(n.toLowerCase()));
  const query = draft.value.trim();
  if (!query) return candidates.slice(0, 8);
  return fuzzyRank(candidates, query, (n) => n, undefined, 8).map((entry) => entry.item);
});

async function refresh(): Promise<void> {
  await loadTaskParticipants(identity.value, true);
}

watch(() => [props.taskKey, props.documentId], () => {
  draft.value = "";
  pickerOpen.value = false;
  void refresh();
}, { immediate: true });

onMounted(() => {
  void loadParticipants();
});

async function add(name: string): Promise<void> {
  const value = name.trim();
  if (!value || busy.value) return;
  busy.value = true;
  try {
    const ok = await addParticipant(identity.value, value);
    if (ok) {
      draft.value = "";
      pickerOpen.value = false;
    }
  } finally {
    busy.value = false;
  }
}

async function remove(name: string): Promise<void> {
  if (busy.value) return;
  busy.value = true;
  try {
    await removeParticipant(identity.value, name);
  } finally {
    busy.value = false;
  }
}

function onInputKeydown(e: KeyboardEvent): void {
  if (e.key === "Enter") {
    e.preventDefault();
    const first = suggestions.value[0];
    void add(draft.value.trim() || first || "");
  } else if (e.key === "Escape") {
    e.preventDefault();
    pickerOpen.value = false;
  } else if (e.key === "ArrowDown" && suggestions.value.length > 0) {
    e.preventDefault();
    draft.value = suggestions.value[0];
  }
}

// ── Bulk assignment ──────────────────────────────────────────────────────────

const bulkTargets = computed<TaskIdentity[]>(() => {
  const keys = Array.from(multiSelectedKeys.value);
  const targets = keys
    .map((key) => getTask(key))
    .filter((t): t is TaskSummary => t !== undefined);
  return targets.map((t) => ({ task_key: t.task_key, document_id: t.document_id }));
});

const bulkCandidates = computed<string[]>(() => {
  const names = new Set(participantSuggestions.value);
  for (const row of rows.value) names.add(row.display_name);
  return Array.from(names).sort((a, b) => a.localeCompare(b));
});

function toggleBulkName(name: string): void {
  const next = new Set(bulkSelection.value);
  if (next.has(name)) next.delete(name);
  else next.add(name);
  bulkSelection.value = next;
}

function openBulk(): void {
  bulkSelection.value = new Set(rows.value.map((r) => r.display_name));
  bulkOpen.value = true;
}

async function applyBulk(replace: boolean): Promise<void> {
  const names = Array.from(bulkSelection.value);
  if (names.length === 0) {
    showToast("Select at least one participant", "warning");
    return;
  }
  if (bulkTargets.value.length === 0) {
    showToast("Ctrl-click tasks in the tree to build a selection", "info");
    return;
  }
  busy.value = true;
  try {
    const result = await bulkAssignParticipants(bulkTargets.value, names, replace);
    showToast(
      `${replace ? "Replaced" : "Added"} participants on ${result.tasks} task(s): `
      + `${result.added} added, ${result.removed} removed, ${result.failed} failed`,
      result.failed > 0 ? "warning" : "success",
    );
    bulkOpen.value = false;
    await loadParticipants(true);
    await refresh();
  } finally {
    busy.value = false;
  }
}

let blurTimer: ReturnType<typeof setTimeout> | null = null;

function onInputBlur(): void {
  // Delay so a click on a suggestion chip still registers before closing.
  if (blurTimer) clearTimeout(blurTimer);
  blurTimer = setTimeout(() => {
    pickerOpen.value = false;
    blurTimer = null;
  }, 150);
}

onBeforeUnmount(() => {
  if (blurTimer) clearTimeout(blurTimer);
  blurTimer = null;
});
</script>

<template>
  <div class="rel-editor">
    <div class="rel-editor__header">
      <span class="rel-editor__title">
        <i class="fas fa-users"></i> Participants
      </span>
      <span class="rel-editor__count">{{ rows.length }}</span>
      <span class="rel-editor__spacer"></span>
      <button
        v-if="multiCount > 0"
        class="btn rel-editor__bulk-btn"
        :disabled="!available || busy"
        title="Assign participants to every selected task"
        @click="openBulk"
      >
        <i class="fas fa-user-plus"></i> Bulk ({{ multiCount }})
      </button>
      <button
        class="rel-editor__icon-btn"
        :disabled="!available"
        title="Reload from the workspace index"
        @click="refresh"
      >
        <i class="fas fa-rotate"></i>
      </button>
    </div>

    <div v-if="rows.length === 0" class="rel-editor__empty">
      No participants yet
    </div>
    <div v-else class="rel-editor__chips">
      <span v-for="row in rows" :key="`${row.display_name}:${row.role}`" class="chip rel-editor__chip">
        <i class="fas fa-user"></i>
        {{ row.display_name }}
        <span v-if="row.role && row.role !== 'allocated_to'" class="rel-editor__role">{{ row.role }}</span>
        <button
          class="rel-editor__icon-btn rel-editor__icon-btn--danger"
          :disabled="!available || busy"
          :title="`Remove ${row.display_name}`"
          @click="remove(row.display_name)"
        >
          <i class="fas fa-xmark"></i>
        </button>
      </span>
    </div>

    <div class="rel-editor__actions">
      <input
        v-model="draft"
        class="rel-editor__input"
        type="text"
        list="participant-suggestions"
        placeholder="Add participant…"
        :disabled="!available || busy"
        @focus="pickerOpen = true"
        @blur="onInputBlur"
        @keydown="onInputKeydown"
      />
      <datalist id="participant-suggestions">
        <option v-for="name in suggestions" :key="name" :value="name" />
      </datalist>
      <button
        class="btn btn-primary"
        :disabled="!available || busy || !draft.trim()"
        @click="add(draft)"
      >
        <i :class="busy ? 'fas fa-spinner fa-spin' : 'fas fa-plus'"></i>
      </button>
    </div>

    <div v-if="pickerOpen && suggestions.length > 0" class="rel-editor__suggest">
      <span class="rel-editor__note">Suggestions:</span>
      <button
        v-for="name in suggestions"
        :key="`s-${name}`"
        class="rel-editor__suggest-chip"
        :disabled="!available || busy"
        @click="add(name)"
      >
        {{ name }}
      </button>
    </div>

    <div v-if="!available" class="rel-editor__note rel-editor__note--warning">
      <i class="fas fa-triangle-exclamation"></i>
      Participant commands are not available in this backend build yet.
    </div>

    <!-- Bulk multi-select assignment -->
    <div v-if="bulkOpen" class="rel-editor__bulk">
      <div class="rel-editor__note">
        Assign to {{ bulkTargets.length }} selected task(s):
      </div>
      <div class="rel-picker">
        <label
          v-for="name in bulkCandidates"
          :key="`bulk-${name}`"
          class="rel-picker__item"
        >
          <input
            type="checkbox"
            :checked="bulkSelection.has(name)"
            @change="toggleBulkName(name)"
          />
          <span class="rel-picker__item-label">{{ name }}</span>
        </label>
        <div v-if="bulkCandidates.length === 0" class="rel-picker__empty">
          No known participants in the workspace index yet.
        </div>
      </div>
      <div class="rel-editor__actions">
        <button class="btn" :disabled="busy" @click="applyBulk(false)">
          <i class="fas fa-plus"></i> Add to selection
        </button>
        <button class="btn" :disabled="busy" @click="applyBulk(true)">
          <i class="fas fa-arrows-rotate"></i> Replace selection
        </button>
        <span class="rel-editor__spacer"></span>
        <button class="btn" :disabled="busy" @click="bulkOpen = false">Close</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.rel-editor__chips {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-1);
}
.rel-editor__chip {
  padding-right: var(--space-1);
}
.rel-editor__role {
  font-size: 10px;
  color: var(--color-text-muted);
  border-left: 1px solid var(--color-border);
  padding-left: var(--space-1);
}
.rel-editor__bulk {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  border-top: 1px dashed var(--color-border);
  padding-top: var(--space-2);
}
.rel-editor__bulk-btn {
  padding: 0 var(--space-2);
  font-size: var(--text-xs);
}
.rel-picker__item input { accent-color: var(--color-accent); }
</style>
