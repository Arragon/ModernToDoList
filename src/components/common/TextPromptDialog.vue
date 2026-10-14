<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import { resolveTextPrompt, textPrompt } from "../../stores/app-state";

const inputEl = ref<HTMLInputElement | null>(null);
const draft = ref("");
const error = ref("");

const visible = computed(() => textPrompt.value.visible);
const options = computed(() => textPrompt.value.options);

watch(visible, async (open) => {
  if (!open) return;
  draft.value = options.value.defaultValue ?? "";
  error.value = "";
  await nextTick();
  inputEl.value?.focus();
  inputEl.value?.select();
});

function confirm(): void {
  const value = draft.value.trim();
  if (!value) {
    error.value = "This field cannot be empty";
    return;
  }
  resolveTextPrompt(value);
}

function cancel(): void {
  resolveTextPrompt(null);
}

function onKeydown(e: KeyboardEvent): void {
  if (e.key === "Enter") {
    e.preventDefault();
    confirm();
  } else if (e.key === "Escape") {
    e.preventDefault();
    cancel();
  }
}
</script>

<template>
  <Teleport to="body">
    <div v-if="visible" class="prompt-overlay" @click.self="cancel">
      <div class="prompt-dialog" role="dialog" aria-modal="true" :aria-label="options.title">
        <h3 class="prompt-dialog__title">{{ options.title }}</h3>
        <p v-if="options.message" class="prompt-dialog__message">{{ options.message }}</p>

        <input
          ref="inputEl"
          v-model="draft"
          class="prompt-dialog__input"
          type="text"
          :placeholder="options.placeholder ?? ''"
          @keydown="onKeydown"
        />
        <p v-if="error" class="prompt-dialog__error">{{ error }}</p>

        <div class="prompt-dialog__actions">
          <button class="btn" @click="cancel">Cancel</button>
          <button class="btn btn-primary" @click="confirm">
            {{ options.confirmLabel ?? 'OK' }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.prompt-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.4);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: var(--z-modal);
}
.prompt-dialog {
  background: var(--color-bg-primary);
  border-radius: var(--radius-xl);
  padding: var(--space-6);
  width: min(480px, 92%);
  box-shadow: var(--shadow-lg);
}
.prompt-dialog__title {
  font-size: var(--text-lg);
  font-weight: 600;
  margin-bottom: var(--space-3);
}
.prompt-dialog__message {
  font-size: var(--text-sm);
  color: var(--color-text-secondary);
  margin-bottom: var(--space-4);
}
.prompt-dialog__input {
  width: 100%;
  padding: var(--space-2) var(--space-3);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
  background: var(--color-bg-primary);
  color: var(--color-text-primary);
  outline: none;
}
.prompt-dialog__input:focus { border-color: var(--color-border-focus); }
.prompt-dialog__error {
  margin-top: var(--space-2);
  font-size: var(--text-xs);
  color: var(--color-error);
}
.prompt-dialog__actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-3);
  margin-top: var(--space-5);
}
</style>
