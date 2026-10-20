<script setup lang="ts">
/**
 * Transfer result banner (RD-M8 / INH-1083).
 *
 * Renders the five terminal states of a transfer: move completed / copy
 * completed / move incomplete (target committed, source retained) / recovery
 * required / failed before target commit. Successful outcomes with durable
 * undo artifacts expose an Undo action.
 */
import { computed, ref } from "vue";
import {
  dismissTransferResult, describeTransferWarning, transferResult, undoTransferById,
} from "../../stores/transfer-store";
import { useT } from "../../app/i18n";

const t = useT();
const undoBusy = ref(false);

interface BannerView {
  tone: "success" | "warning" | "error";
  icon: string;
  title: string;
  message: string;
}

const view = computed<BannerView | null>(() => {
  const result = transferResult.value;
  if (!result) return null;
  switch (result.kind) {
    case "completed":
      return {
        tone: "success",
        icon: "fa-circle-check",
        title: result.operation === "move" ? t.value("transfer.completed") : t.value("transfer.copyCompleted"),
        message: result.outcome
          ? t.value("transfer.completedDetail").replace("{n}", String(result.outcome.target_task_ids.length))
          : "",
      };
    case "recovery_required":
      return {
        tone: "warning",
        icon: "fa-triangle-exclamation",
        title: result.operation === "move" ? t.value("transfer.moveIncomplete") : t.value("transfer.recoveryRequired"),
        message: result.message,
      };
    case "failed_before_target_commit":
      return {
        tone: "error",
        icon: "fa-circle-xmark",
        title: t.value("transfer.failedBeforeCommit"),
        message: `${result.message} ${t.value("transfer.sourceUnchanged")}`,
      };
    case "failed":
      return { tone: "error", icon: "fa-circle-xmark", title: t.value("transfer.failed"), message: result.message };
    default:
      return null;
  }
});

const warnings = computed(() => transferResult.value?.outcome?.warnings ?? []);
const canUndo = computed(() =>
  transferResult.value?.kind === "completed"
  && transferResult.value.outcome?.undo_available === true,
);

async function undo(): Promise<void> {
  const transactionId = transferResult.value?.transactionId;
  if (!transactionId || undoBusy.value) return;
  undoBusy.value = true;
  try {
    await undoTransferById(transactionId);
  } finally {
    undoBusy.value = false;
  }
}
</script>

<template>
  <Teleport to="body">
    <div v-if="view" class="transfer-banner" :class="`transfer-banner--${view.tone}`" role="alert">
      <i class="fas transfer-banner__icon" :class="view.icon"></i>
      <div class="transfer-banner__body">
        <div class="transfer-banner__title">{{ view.title }}</div>
        <div v-if="view.message" class="transfer-banner__message">{{ view.message }}</div>
        <ul v-if="warnings.length > 0" class="transfer-banner__warnings">
          <li v-for="(w, i) in warnings" :key="i">{{ describeTransferWarning(w) }}</li>
        </ul>
      </div>
      <button
        v-if="canUndo"
        class="btn transfer-banner__undo"
        :disabled="undoBusy"
        @click="undo"
      >
        <i class="fas fa-rotate-left"></i> {{ t('action.undo') }}
      </button>
      <button class="transfer-banner__close" :aria-label="t('action.dismiss')" @click="dismissTransferResult()">
        <i class="fas fa-xmark"></i>
      </button>
    </div>
  </Teleport>
</template>

<style scoped>
.transfer-banner {
  position: fixed;
  bottom: var(--space-6);
  right: var(--space-6);
  z-index: var(--z-modal);
  display: flex;
  align-items: flex-start;
  gap: var(--space-3);
  max-width: 480px;
  background: var(--color-bg-primary);
  border: 1px solid var(--color-border);
  border-left-width: 4px;
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-lg);
  padding: var(--space-3) var(--space-4);
}
.transfer-banner--success { border-left-color: var(--color-success); }
.transfer-banner--warning { border-left-color: var(--color-warning); }
.transfer-banner--error { border-left-color: var(--color-error); }
.transfer-banner__icon { margin-top: 2px; }
.transfer-banner--success .transfer-banner__icon { color: var(--color-success); }
.transfer-banner--warning .transfer-banner__icon { color: var(--color-warning); }
.transfer-banner--error .transfer-banner__icon { color: var(--color-error); }
.transfer-banner__body { flex: 1; min-width: 0; }
.transfer-banner__title { font-size: var(--text-sm); font-weight: 600; }
.transfer-banner__message {
  font-size: var(--text-xs);
  color: var(--color-text-secondary);
  margin-top: var(--space-1);
  word-break: break-word;
}
.transfer-banner__warnings {
  font-size: var(--text-xs);
  color: var(--color-text-muted);
  margin-top: var(--space-2);
  padding-left: var(--space-4);
}
.transfer-banner__undo { flex-shrink: 0; }
.transfer-banner__close {
  background: none;
  border: none;
  cursor: pointer;
  color: var(--color-text-muted);
  padding: var(--space-1);
}
.transfer-banner__close:hover { color: var(--color-text-primary); }
</style>
