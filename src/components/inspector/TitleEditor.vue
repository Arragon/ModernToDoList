<script setup lang="ts">
import { onBeforeUnmount, ref, watch } from "vue";
import { selectedTask } from "../../stores/task-store";

withDefaults(defineProps<{
  disabled?: boolean;
}>(), { disabled: false });

const emit = defineEmits<{
  (e: "update:title", value: string): void;
}>();

const localTitle = ref("");
let debounceTimer: ReturnType<typeof setTimeout> | null = null;

watch(selectedTask, (task) => {
  localTitle.value = task?.title ?? "";
  if (debounceTimer) {
    clearTimeout(debounceTimer);
    debounceTimer = null;
  }
}, { immediate: true });

function onInput(e: Event) {
  const val = (e.target as HTMLInputElement).value;
  localTitle.value = val;
  if (debounceTimer) clearTimeout(debounceTimer);
  debounceTimer = setTimeout(() => {
    debounceTimer = null;
    emit("update:title", val);
  }, 400);
}

/** Commits immediately so Enter/blur never loses the last keystrokes. */
function flush() {
  if (debounceTimer) {
    clearTimeout(debounceTimer);
    debounceTimer = null;
    emit("update:title", localTitle.value);
  }
}

onBeforeUnmount(() => {
  if (debounceTimer) clearTimeout(debounceTimer);
  debounceTimer = null;
});
</script>

<template>
  <div class="title-editor">
    <label class="field-label">Title</label>
    <input
      class="title-editor__input"
      type="text"
      :value="localTitle"
      :disabled="disabled"
      @input="onInput"
      @blur="flush"
      @keydown.enter="flush"
      :placeholder="disabled ? 'Editing unavailable' : 'Task title...'"
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
.title-editor__input:disabled {
  color: var(--color-text-disabled);
  background: var(--color-bg-tertiary);
  cursor: not-allowed;
}
</style>
