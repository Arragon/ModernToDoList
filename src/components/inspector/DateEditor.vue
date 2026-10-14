<script setup lang="ts">
withDefaults(defineProps<{
  value: string | null;
  label: string;
  disabled?: boolean;
}>(), { disabled: false });

const emit = defineEmits<{
  (e: "update:value", value: string | null): void;
}>();

function onChange(e: Event) {
  const val = (e.target as HTMLInputElement).value;
  emit("update:value", val || null);
}
</script>

<template>
  <div class="date-editor">
    <label class="field-label">{{ label }}</label>
    <input
      class="date-editor__input"
      type="date"
      :value="value ?? ''"
      :disabled="disabled"
      @change="onChange"
    />
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
.date-editor__input {
  width: 100%;
  padding: var(--space-2) var(--space-3);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
  background: var(--color-bg-primary);
  color: var(--color-text-primary);
  outline: none;
}
.date-editor__input:focus {
  border-color: var(--color-border-focus);
}
.date-editor__input:disabled {
  color: var(--color-text-disabled);
  background: var(--color-bg-tertiary);
  cursor: not-allowed;
}
</style>
