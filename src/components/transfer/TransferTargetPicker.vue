<script setup lang="ts">
/**
 * Transfer target picker (RD-M8 / INH-1083).
 *
 * Lists every other document in the workspace; read-only targets are greyed
 * out. Picking a target runs a side-effect-free preview first — warnings are
 * confirmed explicitly by the store before anything is written. The confirm
 * button copy reflects the pending operation (Copy vs Move).
 */
import { computed, ref } from "vue";
import {
  activePicker, closeTransferPicker, confirmTransfer,
  transferBusy, transferTargets, transferTargetsLoading,
} from "../../stores/transfer-store";
import { documents } from "../../stores/app-state";
import { useT } from "../../app/i18n";

const t = useT();

const picker = computed(() => activePicker.value);
const busy = ref(false);

const rows = computed(() =>
  transferTargets.value.map((target) => {
    const doc = documents.value.find((d) => d.id === target.document_id);
    return {
      ...target,
      exists: Boolean(doc),
      label: target.display_name || doc?.file_path || target.document_id,
    };
  }),
);

async function choose(documentId: string, writable: boolean): Promise<void> {
  if (!writable || busy.value) return;
  busy.value = true;
  try {
    await confirmTransfer(documentId);
  } finally {
    busy.value = false;
  }
}

function cancel(): void {
  closeTransferPicker();
}
</script>

<template>
  <Teleport to="body">
    <div v-if="picker" class="picker-overlay" @click.self="cancel">
      <div class="picker-dialog" role="dialog" :aria-label="t('transfer.pickTarget')">
        <h3 class="picker-dialog__title">
          <i :class="picker.operation === 'move' ? 'fas fa-truck-fast' : 'fas fa-copy'"></i>
          {{ picker.operation === 'move' ? t('transfer.moveTitle') : t('transfer.copyTitle') }}
        </h3>
        <p class="picker-dialog__subtitle">
          "{{ picker.taskTitle || picker.taskKey }}"
        </p>

        <div v-if="transferTargetsLoading" class="picker-dialog__loading">
          <i class="fas fa-spinner fa-spin"></i> {{ t('transfer.loadingTargets') }}
        </div>
        <div v-else-if="rows.length === 0" class="picker-dialog__empty">
          {{ t('transfer.noTargets') }}
        </div>
        <div v-else class="picker-dialog__list">
          <button
            v-for="row in rows"
            :key="row.document_id"
            class="picker-dialog__target"
            :disabled="!row.writable || busy || transferBusy"
            :title="row.writable ? row.path : t('transfer.readOnly')"
            @click="choose(row.document_id, row.writable)"
          >
            <i class="fas fa-file-lines"></i>
            <span class="picker-dialog__target-name truncate-text">{{ row.label }}</span>
            <span class="chip">{{ row.doc_type }}</span>
            <i v-if="!row.writable" class="fas fa-lock picker-dialog__lock"></i>
          </button>
        </div>

        <div class="picker-dialog__actions">
          <button class="btn" @click="cancel">{{ t('action.cancel') }}</button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.picker-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.4);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: var(--z-modal);
}
.picker-dialog {
  background: var(--color-bg-primary);
  border-radius: var(--radius-xl);
  padding: var(--space-6);
  max-width: 440px;
  width: 92%;
  box-shadow: var(--shadow-lg);
}
.picker-dialog__title {
  font-size: var(--text-lg);
  font-weight: 600;
  margin-bottom: var(--space-1);
  display: flex;
  align-items: center;
  gap: var(--space-2);
}
.picker-dialog__subtitle {
  font-size: var(--text-sm);
  color: var(--color-text-muted);
  margin-bottom: var(--space-4);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.picker-dialog__loading,
.picker-dialog__empty {
  padding: var(--space-6);
  text-align: center;
  color: var(--color-text-muted);
  font-size: var(--text-sm);
}
.picker-dialog__list {
  max-height: 320px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}
.picker-dialog__target {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  background: var(--color-bg-secondary);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  cursor: pointer;
  font-size: var(--text-sm);
  color: var(--color-text-primary);
  text-align: left;
}
.picker-dialog__target:hover:not(:disabled) {
  background: var(--color-bg-hover);
  border-color: var(--color-accent);
}
.picker-dialog__target:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.picker-dialog__target-name {
  flex: 1;
  min-width: 0;
}
.picker-dialog__lock {
  font-size: var(--text-xs);
  color: var(--color-text-muted);
}
.picker-dialog__actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-3);
  margin-top: var(--space-4);
}
</style>
