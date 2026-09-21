<script setup lang="ts">
defineProps<{
  value: number;
}>();

const emit = defineEmits<{
  (e: "update:value", value: number): void;
}>();

const priorities = [
  { value: 0, label: "None" },
  { value: 1, label: "Low" },
  { value: 2, label: "Medium" },
  { value: 3, label: "High" },
  { value: 4, label: "Very High" },
];

function onChange(e: Event) {
  emit("update:value", Number((e.target as HTMLSelectElement).value));
}
</script>

<template>
  <div class="priority-editor">
    <label class="field-label">Priority</label>
    <select class="priority-editor__select" :value="value" @change="onChange">
      <option v-for="p in priorities" :key="p.value" :value="p.value">{{ p.label }}</option>
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
.priority-editor__select {
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
.priority-editor__select:focus {
  border-color: var(--color-border-focus);
}
</style>
