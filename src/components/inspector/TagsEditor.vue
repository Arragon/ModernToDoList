<script setup lang="ts">
import { ref } from "vue";

const props = withDefaults(defineProps<{
  tags: string[];
  label: string;
  disabled?: boolean;
}>(), { disabled: false });

const emit = defineEmits<{
  (e: "update:tags", tags: string[]): void;
}>();

const newTag = ref("");

function addTag() {
  if (props.disabled) return;
  const tag = newTag.value.trim();
  if (tag) {
    emit("update:tags", [...props.tags, tag]);
    newTag.value = "";
  }
}

function removeTag(index: number) {
  if (props.disabled) return;
  const updated = props.tags.filter((_, i) => i !== index);
  emit("update:tags", updated);
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === "Enter") {
    e.preventDefault();
    addTag();
  }
}
</script>

<template>
  <div class="tags-editor">
    <label class="field-label">{{ label }}</label>
    <div class="tags-editor__chips">
      <span v-for="(tag, i) in tags" :key="i" class="tags-editor__chip">
        {{ tag }}
        <button class="tags-editor__remove" :disabled="disabled" @click="removeTag(i)">&times;</button>
      </span>
    </div>
    <input
      class="tags-editor__input"
      type="text"
      v-model="newTag"
      :disabled="disabled"
      @keydown="onKeydown"
      :placeholder="disabled ? 'Tag editing unavailable' : `Add ${label.toLowerCase()}...`"
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
.tags-editor__chips {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-1);
  margin-bottom: var(--space-2);
}
.tags-editor__chip {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  padding: 2px var(--space-2);
  background: var(--color-bg-tertiary);
  border-radius: var(--radius-full);
  font-size: var(--text-xs);
  color: var(--color-text-secondary);
}
.tags-editor__remove {
  background: none;
  border: none;
  cursor: pointer;
  color: var(--color-text-muted);
  font-size: var(--text-sm);
  padding: 0 2px;
  line-height: 1;
}
.tags-editor__remove:hover {
  color: var(--color-error);
}
.tags-editor__input {
  width: 100%;
  padding: var(--space-2) var(--space-3);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
  background: var(--color-bg-primary);
  color: var(--color-text-primary);
  outline: none;
}
.tags-editor__input:focus {
  border-color: var(--color-border-focus);
}
.tags-editor__input:disabled {
  color: var(--color-text-disabled);
  background: var(--color-bg-tertiary);
  cursor: not-allowed;
}
.tags-editor__remove:disabled {
  cursor: not-allowed;
  opacity: 0.5;
}
</style>
