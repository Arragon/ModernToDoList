<script setup lang="ts">
import { onBeforeUnmount, onMounted, watch } from "vue";
import MainLayout from "./components/layout/MainLayout.vue";
import Toast from "./components/common/Toast.vue";
import ConfirmDialog from "./components/common/ConfirmDialog.vue";
import LoadingState from "./components/common/LoadingState.vue";
import TransferTargetPicker from "./components/transfer/TransferTargetPicker.vue";
import TransferProgressToast from "./components/transfer/TransferProgressToast.vue";
import TransferResultBanner from "./components/transfer/TransferResultBanner.vue";
import TransferRecoveryPanel from "./components/transfer/TransferRecoveryPanel.vue";
import { appLoading, appLoadingMessage, workspace } from "./stores/app-state";
import { ping, getRuntimeInfo } from "./ipc/client";
import { initKeyboardShortcuts, disposeKeyboardShortcuts } from "./app/shortcuts";
import { installPerfBridge } from "./app/perf";
import { loadSavedViews } from "./stores/saved-view-store";
import {
  disposeTransferProgressListener, initTransferProgressListener,
  resetTransferState, runRecoveryScan,
} from "./stores/transfer-store";

onMounted(async () => {
  // RD-M10-016/022: publish the render-measurement hooks before any tree renders.
  installPerfBridge();
  initKeyboardShortcuts();
  void loadSavedViews();
  void initTransferProgressListener();
  try {
    await ping();
    await getRuntimeInfo();
  } catch (e) {
    // The backend may not be reachable yet (dev server without Tauri); the UI
    // still renders and every command degrades through the capability store.
    console.warn("IPC not available:", e);
  }
});

// RD-M8: scan the transfer journal once a workspace is open so interrupted
// transactions surface immediately instead of lingering silently.
watch(() => workspace.value, (ws) => {
  if (ws) void runRecoveryScan();
  else resetTransferState();
});

onBeforeUnmount(() => {
  disposeKeyboardShortcuts();
  disposeTransferProgressListener();
});
</script>

<template>
  <div class="app-root">
    <LoadingState v-if="appLoading" :message="appLoadingMessage" />
    <MainLayout v-else />
    <Toast />
    <ConfirmDialog />
    <TransferTargetPicker />
    <TransferProgressToast />
    <TransferResultBanner />
    <TransferRecoveryPanel />
  </div>
</template>

<style scoped>
.app-root { height: 100%; width: 100%; overflow: hidden; }
</style>
