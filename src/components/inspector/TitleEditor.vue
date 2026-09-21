<script setup lang="ts">
import { ref, watch } from "vue";
import { selectedTask } from "../../stores/task-store";

const emit = defineEmits<{
  (e: "update:title", value: string): void;
}>();

const localTitle = ref("");
let debounceTimer: ReturnType<typeof setTimeout> | null = null;

watch(selectedTask, (task) => {
  localTitle.value = task?.title ?? "";
}, { immediate: true });

function onInput(e: Event) {
  const val = (e.target as HTMLInputElement).value;
  localTitle.value = val;
  if (debounceTimer) clearTimeout(debounceTimer);
  debounceTimer = setTimeout(() => {
    emit("update:title", val);
  }, 400);
}
</script>

<template>
  <div class="title-editor">
    <label class="field-label">Title</label>
    <input
      class="title-editor__input"
      type="text"
      :value="localTitle"
      @input="onInput"
      placeholder="Task title..."
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
.title-editor__input {
  width: 100%;
  padding: var(--space-2) var(--space-3);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  font-size: var(--text-base);
  font-weight: 500;
  background: var(--color-bg-primary);
  color: var(--color-text-primary);
  outline: none;
  transition: border-color var(--transition-fast);
}
.title-editor__input:focus {
  border-color: var(--color-border-focus);
}
</style>
