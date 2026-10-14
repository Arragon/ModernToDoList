<script setup lang="ts">
/**
 * Secondary task properties: identity, progress, risk and completion date.
 * Progress/risk/completion are editable and route through `update_task_field`.
 */
import { computed, ref } from "vue";
import { isDirty, currentRevision, hasSessionFor } from "../../stores/session-store";

const props = withDefaults(defineProps<{
  percentDone: number;
  risk: number;
  completedDate: string | null;
  taskKey: string;
  documentId: string;
  disabled?: boolean;
}>(), { disabled: false });

const emit = defineEmits<{
  (e: "update:percentDone", value: number): void;
  (e: "update:risk", value: number): void;
  (e: "update:completedDate", value: string | null): void;
}>();

const expanded = ref(false);

const sessionOpen = computed(() => hasSessionFor(props.documentId));

function toggle() {
  expanded.value = !expanded.value;
}

function clampInt(raw: string, min: number, max: number): number {
  const parsed = Number.parseInt(raw, 10);
  if (!Number.isFinite(parsed)) return min;
  return Math.min(max, Math.max(min, parsed));
}

function onPercentChange(e: Event) {
  const value = clampInt((e.target as HTMLInputElement).value, 0, 100);
  (e.target as HTMLInputElement).value = String(value);
  emit("update:percentDone", value);
}

function onRiskChange(e: Event) {
  emit("update:risk", clampInt((e.target as HTMLSelectElement).value, 0, 4));
}

function onCompletedChange(e: Event) {
  const value = (e.target as HTMLInputElement).value;
  emit("update:completedDate", value || null);
}
</script>

<template>
  <div class="more-properties">
    <button class="more-properties__toggle" @click="toggle">
      <i :class="expanded ? 'fas fa-chevron-down' : 'fas fa-chevron-right'"></i>
      More Properties
    </button>
    <div v-if="expanded" class="more-properties__content">
      <div class="more-properties__field">
        <label class="field-label">Task Key</label>
        <span class="field-value">{{ taskKey }}</span>
      </div>
      <div class="more-properties__field">
        <label class="field-label">Document ID</label>
        <span class="field-value">{{ documentId }}</span>
      </div>

      <div class="more-properties__field">
        <label class="field-label" for="percent-done">Percent Done</label>
        <input
          id="percent-done"
          class="more-properties__input"
          type="number"
          min="0"
          max="100"
          step="5"
          :value="percentDone"
          :disabled="disabled"
          @change="onPercentChange"
        />
      </div>

      <div class="more-properties__field">
        <label class="field-label" for="risk-level">Risk</label>
        <select
          id="risk-level"
          class="more-properties__input"
          :value="risk"
          :disabled="disabled"
          @change="onRiskChange"
        >
          <option :value="0">0 — None</option>
          <option :value="1">1 — Low</option>
          <option :value="2">2 — Medium</option>
          <option :value="3">3 — High</option>
          <option :value="4">4 — Critical</option>
        </select>
      </div>

      <div class="more-properties__field">
        <label class="field-label" for="completed-date">Completed Date</label>
        <input
          id="completed-date"
          class="more-properties__input"
          type="date"
          :value="completedDate ?? ''"
          :disabled="disabled"
          @change="onCompletedChange"
        />
      </div>

      <div class="more-properties__field">
        <label class="field-label">Session</label>
        <span class="field-value more-properties__session">
          <span
            :class="['status-dot', sessionOpen ? 'status-dot--ok' : 'status-dot--off']"
          ></span>
          {{ sessionOpen ? `open · rev ${currentRevision}` : 'not open' }}
          <span v-if="isDirty" class="more-properties__dirty">unsaved changes</span>
        </span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.more-properties {
  border-top: 1px solid var(--color-border);
  padding-top: var(--space-3);
}
.more-properties__toggle {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  background: none;
  border: none;
  cursor: pointer;
  font-size: var(--text-sm);
  font-weight: 600;
  color: var(--color-text-secondary);
  padding: var(--space-1) 0;
  width: 100%;
  text-align: left;
}
.more-properties__toggle:hover {
  color: var(--color-text-primary);
}
.more-properties__content {
  padding-top: var(--space-3);
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}
.more-properties__field {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.field-label {
  font-size: var(--text-xs);
  font-weight: 600;
  color: var(--color-text-secondary);
  text-transform: uppercase;
  letter-spacing: 0.05em;
}
.field-value {
  font-size: var(--text-sm);
  color: var(--color-text-primary);
  font-family: var(--font-mono);
  word-break: break-all;
}
.more-properties__input {
  padding: var(--space-1) var(--space-2);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
  background: var(--color-bg-primary);
  color: var(--color-text-primary);
  outline: none;
}
.more-properties__input:focus { border-color: var(--color-border-focus); }
.more-properties__input:disabled {
  color: var(--color-text-disabled);
  background: var(--color-bg-tertiary);
  cursor: not-allowed;
}
.more-properties__session {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}
.more-properties__dirty {
  color: var(--color-warning);
  font-family: var(--font-sans);
  font-size: var(--text-xs);
}
.status-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
  display: inline-block;
}
.status-dot--ok { background: var(--color-success); }
.status-dot--off { background: var(--color-text-disabled); }
</style>
