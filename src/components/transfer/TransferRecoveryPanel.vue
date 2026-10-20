<script setup lang="ts">
/**
 * Transfer recovery panel (RD-M8 / INH-1083).
 *
 * Lists interrupted transfer transactions recovered from the journal. A
 * `completed_with_duplicate` report — target committed, source retained —
 * offers the two safe resolutions: complete the move (delete the source
 * copy, explicit confirm) or keep the duplicate. Rolled-back/quarantined
 * reports are informational and can be dismissed.
 */
import { ref } from "vue";
import type { TransferRecoveryReportDto } from "../../ipc/types";
import {
  completeRecovery, keepRecoveryDuplicate, recoveryReports,
} from "../../stores/transfer-store";
import { showConfirm } from "../../stores/app-state";
import { useT } from "../../app/i18n";

const t = useT();
const busyId = ref<string | null>(null);

function reportKey(report: TransferRecoveryReportDto, index: number): string {
  return report.transaction_id ?? `journal-${index}`;
}

function actionLabel(report: TransferRecoveryReportDto): string {
  const key = `transfer.recovery.action.${report.action}`;
  const label = t.value(key);
  return label === key ? report.action : label;
}

function confirmComplete(report: TransferRecoveryReportDto): void {
  if (!report.transaction_id || busyId.value) return;
  const transactionId = report.transaction_id;
  showConfirm(
    t.value("transfer.recovery.completeTitle"),
    t.value("transfer.recovery.completeMessage"),
    () => {
      busyId.value = transactionId;
      void completeRecovery(transactionId).finally(() => {
        busyId.value = null;
      });
    },
    t.value("transfer.recovery.completeAction"),
  );
}
</script>

<template>
  <Teleport to="body">
    <div v-if="recoveryReports.length > 0" class="recovery-panel" role="alert">
      <div class="recovery-panel__header">
        <i class="fas fa-triangle-exclamation"></i>
        <span>{{ t('transfer.recovery.title') }} ({{ recoveryReports.length }})</span>
      </div>
      <div
        v-for="(report, i) in recoveryReports"
        :key="reportKey(report, i)"
        class="recovery-panel__item"
      >
        <div class="recovery-panel__item-head">
          <span class="recovery-panel__action" :class="`recovery-panel__action--${report.action}`">
            {{ actionLabel(report) }}
          </span>
          <span v-if="report.operation" class="recovery-panel__op">
            {{ report.operation === 'move' ? t('transfer.moveTitle') : t('transfer.copyTitle') }}
          </span>
        </div>
        <div class="recovery-panel__detail">{{ report.detail }}</div>
        <div
          v-if="report.action === 'completed_with_duplicate' && report.transaction_id"
          class="recovery-panel__actions"
        >
          <button
            class="btn btn-primary"
            :disabled="busyId === report.transaction_id"
            @click="confirmComplete(report)"
          >
            <i class="fas fa-check"></i> {{ t('transfer.recovery.completeAction') }}
          </button>
          <button
            class="btn"
            :disabled="busyId === report.transaction_id"
            @click="keepRecoveryDuplicate(report.transaction_id!)"
          >
            <i class="fas fa-clone"></i> {{ t('transfer.recovery.keepDuplicate') }}
          </button>
        </div>
        <div v-else-if="report.action !== 'completed_with_duplicate'" class="recovery-panel__actions">
          <button
            v-if="report.transaction_id"
            class="btn"
            @click="keepRecoveryDuplicate(report.transaction_id)"
          >
            {{ t('action.dismiss') }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.recovery-panel {
  position: fixed;
  bottom: var(--space-6);
  left: var(--space-6);
  z-index: var(--z-modal);
  max-width: 440px;
  background: var(--color-bg-primary);
  border: 1px solid var(--color-warning);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-lg);
  padding: var(--space-3) var(--space-4);
}
.recovery-panel__header {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-sm);
  font-weight: 600;
  color: var(--color-warning);
  margin-bottom: var(--space-2);
}
.recovery-panel__item {
  border-top: 1px solid var(--color-border);
  padding: var(--space-2) 0;
}
.recovery-panel__item-head {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}
.recovery-panel__action {
  font-size: var(--text-xs);
  font-weight: 600;
  padding: 0 var(--space-1);
  border-radius: var(--radius-sm);
  background: var(--color-bg-tertiary);
}
.recovery-panel__action--completed_with_duplicate { color: var(--color-warning); }
.recovery-panel__action--rolled_back { color: var(--color-success); }
.recovery-panel__action--quarantined { color: var(--color-error); }
.recovery-panel__op {
  font-size: var(--text-xs);
  color: var(--color-text-muted);
}
.recovery-panel__detail {
  font-size: var(--text-xs);
  color: var(--color-text-secondary);
  margin: var(--space-1) 0;
  word-break: break-word;
}
.recovery-panel__actions {
  display: flex;
  gap: var(--space-2);
  margin-top: var(--space-2);
}
</style>
