<script setup lang="ts">
defineProps<{
  value: string;
}>();

const emit = defineEmits<{
  (e: "update:value", value: string): void;
}>();

const statuses = ["Not Started", "In Progress", "Completed", "Blocked"];

function onChange(e: Event) {
  emit("update:value", (e.target as HTMLSelectElement).value);
}
</script>

<template>
  <div class="status-editor">
    <label class="field-label">Status</label>
    <select class="status-editor__select" :value="value" @change="onChange">
      <option v-for="s in statuses" :key="s" :value="s">{{ s }}</option>
    </select>
  </div>
</template>

<style scoped>
.field-label {
  display: block;
  font-size: var(--text-xs);
  font-weight: 600;
  color: var(--color-text-secondary);
  margin-bottom: var(--space-1);
  text-transform: uppercase;
  letter-spacing: 0.05em;
}
.status-editor__select {
  width: 100%;
  padding: var(--space-2) var(--space-3);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
  background: var(--color-bg-primary);
  color: var(--color-text-primary);
  outline: none;
  cursor: pointer;
}
.status-editor__select:focus {
  border-color: var(--color-border-focus);
}
</style>
