<script setup lang="ts">
import { onBeforeUnmount, onMounted } from "vue";
import { confirmDialog } from "../../stores/app-state";
import { t } from "../../app/i18n";

function handleConfirm() {
  if (confirmDialog.value.onConfirm) {
    confirmDialog.value.onConfirm();
  }
  confirmDialog.value.visible = false;
  confirmDialog.value.onConfirm = null;
}

function handleCancel() {
  confirmDialog.value.visible = false;
  confirmDialog.value.onConfirm = null;
}

function onKeydown(e: KeyboardEvent): void {
  if (e.key === "Escape" && confirmDialog.value.visible) {
    e.preventDefault();
    e.stopPropagation();
    handleCancel();
  }
}

onMounted(() => window.addEventListener("keydown", onKeydown, true));
onBeforeUnmount(() => window.removeEventListener("keydown", onKeydown, true));
</script>

<template>
  <Teleport to="body">
    <div v-if="confirmDialog.visible" class="confirm-overlay" @click.self="handleCancel">
      <div class="confirm-dialog" role="dialog" aria-modal="true" :aria-label="confirmDialog.title">
        <h3 class="confirm-dialog__title">{{ confirmDialog.title }}</h3>
        <p class="confirm-dialog__message">{{ confirmDialog.message }}</p>
        <div class="confirm-dialog__actions">
          <button class="btn" @click="handleCancel">{{ t('dialog.cancel') }}</button>
          <button class="btn btn-danger" @click="handleConfirm">{{ confirmDialog.confirmLabel }}</button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.confirm-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.4);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: var(--z-modal);
}
.confirm-dialog {
  background: var(--color-bg-primary);
  border-radius: var(--radius-xl);
  padding: var(--space-6);
  max-width: 400px;
  width: 90%;
  box-shadow: var(--shadow-lg);
}
.confirm-dialog__title { font-size: var(--text-lg); font-weight: 600; margin-bottom: var(--space-3); }
.confirm-dialog__message { font-size: var(--text-sm); color: var(--color-text-secondary); margin-bottom: var(--space-6); }
.confirm-dialog__actions { display: flex; justify-content: flex-end; gap: var(--space-3); }
</style>
