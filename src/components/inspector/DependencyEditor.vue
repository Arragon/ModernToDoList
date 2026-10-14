<script setup lang="ts">
/**
 * Dependency editor (RD-M6-017~022).
 *
 * Outgoing edges ("依赖 / Depends on") and incoming edges ("阻塞了 / Blocks")
 * live in separate sections, the picker searches the loaded task index, and
 * unresolved / circular references render in degraded visual states.
 */
import { computed, onBeforeUnmount, ref, watch } from "vue";
import type { DependencyDto, TaskSummary } from "../../ipc/types";
import {
  addDependency, classifyDependency, dependenciesAvailable, incomingDependencies,
  loadDependencies, loadTaskDependencies, outgoingDependencies, removeDependency,
} from "../../stores/relation-store";
import type { DependencyState, TaskIdentity } from "../../stores/relation-store";
import { allTasks, getTask, jumpToTask } from "../../stores/task-store";
import { showToast } from "../../stores/app-state";
import { fuzzyRank } from "../../app/fuzzy";

const props = defineProps<{
  taskKey: string;
  documentId: string;
}>();

const PICKER_LIMIT = 25;

const identity = computed<TaskIdentity>(() => ({
  task_key: props.taskKey,
  document_id: props.documentId,
}));

const query = ref("");
const pickerOpen = ref(false);
const activePick = ref(0);
const busy = ref(false);

const available = computed(() => dependenciesAvailable.value);
const outgoing = computed<DependencyDto[]>(() => outgoingDependencies(props.taskKey));
const incoming = computed<DependencyDto[]>(() => incomingDependencies(props.taskKey));

/** Candidate targets: everything except the task itself and current edges. */
const candidates = computed<TaskSummary[]>(() => {
  const excluded = new Set<string>(outgoing.value.map((d) => d.depends_on_key));
  excluded.add(props.taskKey);
  const pool = allTasks.value.filter((t) => !excluded.has(t.task_key));
  const q = query.value.trim();
  if (!q) return pool.slice(0, PICKER_LIMIT);
  return fuzzyRank(pool, q, (t) => t.title, (t) => `${t.task_key} ${t.status}`, PICKER_LIMIT)
    .map((entry) => entry.item);
});

async function refresh(): Promise<void> {
  await Promise.all([
    loadTaskDependencies(identity.value, true),
    loadDependencies(),
  ]);
}

watch(() => [props.taskKey, props.documentId], () => {
  query.value = "";
  pickerOpen.value = false;
  activePick.value = 0;
  void refresh();
}, { immediate: true });

watch(query, () => {
  activePick.value = 0;
});

let blurTimer: ReturnType<typeof setTimeout> | null = null;

function onBlur(): void {
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

function stateOf(dep: DependencyDto): DependencyState {
  return classifyDependency(dep, (key) => getTask(key) !== undefined);
}

function titleOf(taskKey: string): string {
  return getTask(taskKey)?.title || taskKey;
}

function statusOf(taskKey: string): string {
  return getTask(taskKey)?.status || "—";
}

function stateIcon(state: DependencyState): string {
  switch (state) {
    case "circular": return "fa-rotate";
    case "unresolved": return "fa-circle-question";
    case "external": return "fa-globe";
    default: return "fa-arrow-right-long";
  }
}

function stateLabel(state: DependencyState): string {
  switch (state) {
    case "circular": return "Circular reference";
    case "unresolved": return "Unresolved reference";
    case "external": return "External document";
    default: return "";
  }
}

function rowClass(state: DependencyState): string {
  if (state === "circular") return "rel-editor__row rel-editor__row--error";
  if (state === "unresolved") return "rel-editor__row rel-editor__row--warning";
  return "rel-editor__row";
}

function depTypeLabel(depType: number): string {
  switch (depType) {
    case 0: return "FS";
    case 1: return "SS";
    case 2: return "FF";
    case 3: return "SF";
    default: return `T${depType}`;
  }
}

async function add(target: TaskSummary): Promise<void> {
  if (busy.value) return;
  busy.value = true;
  try {
    const ok = await addDependency(identity.value, {
      task_key: target.task_key,
      document_id: target.document_id,
    });
    if (ok) {
      query.value = "";
      pickerOpen.value = false;
      await refresh();
    }
  } finally {
    busy.value = false;
  }
}

async function remove(dep: DependencyDto): Promise<void> {
  if (busy.value) return;
  busy.value = true;
  try {
    const ok = await removeDependency(dep);
    if (ok) {
      await refresh();
      showToast("Dependency removed", "success");
    }
  } finally {
    busy.value = false;
  }
}

function jump(taskKey: string): void {
  if (!getTask(taskKey)) {
    showToast("That task is not in the loaded index", "warning");
    return;
  }
  jumpToTask(taskKey);
}

function onPickerKeydown(e: KeyboardEvent): void {
  const count = candidates.value.length;
  if (e.key === "ArrowDown") {
    e.preventDefault();
    pickerOpen.value = true;
    activePick.value = count === 0 ? 0 : (activePick.value + 1) % count;
  } else if (e.key === "ArrowUp") {
    e.preventDefault();
    activePick.value = count === 0 ? 0 : (activePick.value - 1 + count) % count;
  } else if (e.key === "Enter") {
    e.preventDefault();
    const target = candidates.value[activePick.value];
    if (target) void add(target);
  } else if (e.key === "Escape") {
    e.preventDefault();
    pickerOpen.value = false;
  }
}
</script>

<template>
  <div class="rel-editor">
    <div class="rel-editor__header">
      <span class="rel-editor__title">
        <i class="fas fa-diagram-project"></i> Dependencies
      </span>
      <span class="rel-editor__count">{{ outgoing.length + incoming.length }}</span>
      <span class="rel-editor__spacer"></span>
      <button
        class="rel-editor__icon-btn"
        :disabled="!available"
        title="Reload dependency graph"
        @click="refresh"
      >
        <i class="fas fa-rotate"></i>
      </button>
    </div>

    <!-- 依赖 / Depends on (outgoing) -->
    <div class="dep-section">
      <div class="dep-section__title">
        <i class="fas fa-arrow-right-long"></i> 依赖 · Depends on
        <span class="rel-editor__count">{{ outgoing.length }}</span>
      </div>
      <div v-if="outgoing.length === 0" class="rel-editor__empty">
        This task does not depend on anything.
      </div>
      <div v-else class="rel-editor__list">
        <div
          v-for="dep in outgoing"
          :key="`out-${dep.depends_on_key}`"
          :class="rowClass(stateOf(dep))"
        >
          <i :class="['rel-editor__state', 'fas', stateIcon(stateOf(dep))]"></i>
          <span class="rel-editor__row-label" :title="titleOf(dep.depends_on_key)">
            {{ titleOf(dep.depends_on_key) }}
          </span>
          <span class="chip" :title="`Dependency type ${dep.dep_type}`">
            {{ depTypeLabel(dep.dep_type) }}
          </span>
          <span v-if="stateOf(dep) !== 'ok'" class="rel-editor__state-label">
            {{ stateLabel(stateOf(dep)) }}
          </span>
          <button
            class="rel-editor__icon-btn"
            title="Show in tree"
            :disabled="stateOf(dep) !== 'ok'"
            @click="jump(dep.depends_on_key)"
          >
            <i class="fas fa-location-crosshairs"></i>
          </button>
          <button
            class="rel-editor__icon-btn rel-editor__icon-btn--danger"
            title="Remove dependency"
            :disabled="!available || busy"
            @click="remove(dep)"
          >
            <i class="fas fa-xmark"></i>
          </button>
        </div>
      </div>
    </div>

    <!-- 阻塞了 / Blocks (incoming) -->
    <div class="dep-section">
      <div class="dep-section__title">
        <i class="fas fa-hand"></i> 阻塞了 · Blocks
        <span class="rel-editor__count">{{ incoming.length }}</span>
      </div>
      <div v-if="incoming.length === 0" class="rel-editor__empty">
        No other task is waiting on this one.
      </div>
      <div v-else class="rel-editor__list">
        <div
          v-for="dep in incoming"
          :key="`in-${dep.task_key}`"
          :class="rowClass(stateOf(dep))"
        >
          <i :class="['rel-editor__state', 'fas', stateIcon(stateOf(dep))]"></i>
          <span class="rel-editor__row-label" :title="titleOf(dep.task_key)">
            {{ titleOf(dep.task_key) }}
          </span>
          <span class="rel-editor__row-sub">{{ statusOf(dep.task_key) }}</span>
          <span v-if="stateOf(dep) !== 'ok'" class="rel-editor__state-label">
            {{ stateLabel(stateOf(dep)) }}
          </span>
          <button
            class="rel-editor__icon-btn"
            title="Show in tree"
            :disabled="stateOf(dep) !== 'ok'"
            @click="jump(dep.task_key)"
          >
            <i class="fas fa-location-crosshairs"></i>
          </button>
          <button
            class="rel-editor__icon-btn rel-editor__icon-btn--danger"
            title="Remove dependency"
            :disabled="!available || busy"
            @click="remove(dep)"
          >
            <i class="fas fa-xmark"></i>
          </button>
        </div>
      </div>
    </div>

    <!-- Picker -->
    <div class="rel-editor__actions">
      <input
        v-model="query"
        class="rel-editor__input"
        type="text"
        placeholder="Search a task to depend on…"
        :disabled="!available || busy"
        @focus="pickerOpen = true"
        @blur="onBlur"
        @keydown="onPickerKeydown"
      />
      <button
        class="btn"
        :disabled="!available || busy || candidates.length === 0"
        @click="add(candidates[activePick] ?? candidates[0])"
      >
        <i :class="busy ? 'fas fa-spinner fa-spin' : 'fas fa-plus'"></i> Add
      </button>
    </div>

    <div v-if="pickerOpen && query.trim()" class="rel-picker">
      <button
        v-for="(candidate, index) in candidates"
        :key="candidate.task_key"
        class="rel-picker__item"
        :class="{ 'rel-picker__item--active': index === activePick }"
        @mousedown.prevent="add(candidate)"
      >
        <span class="rel-picker__item-label">{{ candidate.title || "(Untitled)" }}</span>
        <span class="rel-picker__item-meta">{{ candidate.task_key }}</span>
      </button>
      <div v-if="candidates.length === 0" class="rel-picker__empty">No matching task.</div>
    </div>

    <div v-if="!available" class="rel-editor__note rel-editor__note--warning">
      <i class="fas fa-triangle-exclamation"></i>
      Dependency commands are not available in this backend build yet.
    </div>
  </div>
</template>

<style scoped>
.dep-section {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}
.dep-section__title {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-xs);
  font-weight: 600;
  color: var(--color-text-muted);
}
.rel-editor__state {
  width: 14px;
  text-align: center;
  color: var(--color-text-muted);
  flex-shrink: 0;
}
.rel-editor__row--warning .rel-editor__state { color: var(--color-warning); }
.rel-editor__row--error .rel-editor__state { color: var(--color-error); }
.rel-editor__state-label {
  font-size: var(--text-xs);
  color: var(--color-text-muted);
  flex-shrink: 0;
}
.rel-editor__row--warning .rel-editor__state-label { color: var(--color-warning); }
.rel-editor__row--error .rel-editor__state-label { color: var(--color-error); }
</style>
