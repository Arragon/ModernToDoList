<script setup lang="ts">
import { toasts, dismissToast } from "../../stores/app-state";
</script>

<template>
  <Teleport to="body">
    <div v-if="toasts.length > 0" class="toast-container">
      <div
        v-for="toast in toasts"
        :key="toast.id"
        :class="['toast', `toast--${toast.type}`]"
        @click="dismissToast(toast.id)"
      >
        <i :class="['fas', {
          'fa-circle-info': toast.type === 'info',
          'fa-circle-check': toast.type === 'success',
          'fa-triangle-exclamation': toast.type === 'warning',
          'fa-circle-xmark': toast.type === 'error',
        }]"></i>
        <span>{{ toast.message }}</span>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.toast-container {
  position: fixed;
  bottom: var(--space-6);
  right: var(--space-6);
  z-index: var(--z-toast);
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  max-width: 400px;
}
.toast {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-3) var(--space-4);
  border-radius: var(--radius-lg);
  background: var(--color-bg-primary);
  box-shadow: var(--shadow-lg);
  border-left: 4px solid var(--color-info);
  font-size: var(--text-sm);
  cursor: pointer;
}
.toast--success { border-left-color: var(--color-success); }
.toast--warning { border-left-color: var(--color-warning); }
.toast--error { border-left-color: var(--color-error); }
.toast i { flex-shrink: 0; }
.toast--info i { color: var(--color-info); }
.toast--success i { color: var(--color-success); }
.toast--warning i { color: var(--color-warning); }
.toast--error i { color: var(--color-error); }
</style>
