<script setup lang="ts">
/**
 * Live transfer progress toast (RD-M8 / INH-1083).
 *
 * Driven by `transfer://progress` events; renders one non-dismissable card
 * while a transfer runs. Asset staging shows a done/total counter.
 */
import { computed } from "vue";
import { activeTransfer } from "../../stores/transfer-store";
import { useT } from "../../app/i18n";

const t = useT();

const phaseLabel = computed(() => {
  const active = activeTransfer.value;
  if (!active) return "";
  const key = `transfer.phase.${active.phase}`;
  const label = t.value(key);
  return label === key ? active.phase : label;
});
</script>

<template>
  <Teleport to="body">
    <div v-if="activeTransfer" class="transfer-progress" role="status">
      <i class="fas fa-spinner fa-spin"></i>
      <span class="transfer-progress__label">
        {{ activeTransfer.operation === 'move' ? t('transfer.moving') : t('transfer.copying') }}
        — {{ phaseLabel }}
        <template v-if="activeTransfer.detail">{{ activeTransfer.detail }}</template>
      </span>
    </div>
  </Teleport>
</template>

<style scoped>
.transfer-progress {
  position: fixed;
  bottom: var(--space-6);
  right: var(--space-6);
  z-index: var(--z-modal);
  display: flex;
  align-items: center;
  gap: var(--space-2);
  background: var(--color-bg-primary);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-lg);
  padding: var(--space-3) var(--space-4);
  font-size: var(--text-sm);
  color: var(--color-text-primary);
}
.transfer-progress__label {
  display: flex;
  align-items: center;
  gap: var(--space-1);
}
</style>
