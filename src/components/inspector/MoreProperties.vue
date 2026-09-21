<script setup lang="ts">
import { ref } from "vue";

defineProps<{
  percentDone: number;
  risk: number;
  completedDate: string | null;
  taskKey: string;
  documentId: string;
}>();

const expanded = ref(false);

function toggle() {
  expanded.value = !expanded.value;
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
        <label class="field-label">Risk</label>
        <span class="field-value">{{ risk }}</span>
      </div>
      <div class="more-properties__field">
        <label class="field-label">Completed Date</label>
        <span class="field-value">{{ completedDate || '—' }}</span>
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
</style>
