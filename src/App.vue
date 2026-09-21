<script setup lang="ts">
import { onBeforeUnmount, onMounted } from "vue";
import MainLayout from "./components/layout/MainLayout.vue";
import Toast from "./components/common/Toast.vue";
import ConfirmDialog from "./components/common/ConfirmDialog.vue";
import LoadingState from "./components/common/LoadingState.vue";
import { appLoading, appLoadingMessage } from "./stores/app-state";
import { ping, getRuntimeInfo } from "./ipc/client";
import { initKeyboardShortcuts, disposeKeyboardShortcuts } from "./app/shortcuts";
import { installPerfBridge } from "./app/perf";
import { loadSavedViews } from "./stores/saved-view-store";

onMounted(async () => {
  // RD-M10-016/022: publish the render-measurement hooks before any tree renders.
  installPerfBridge();
  initKeyboardShortcuts();
  void loadSavedViews();
  try {
    await ping();
    await getRuntimeInfo();
  } catch (e) {
    // The backend may not be reachable yet (dev server without Tauri); the UI
    // still renders and every command degrades through the capability store.
    console.warn("IPC not available:", e);
  }
});

onBeforeUnmount(() => {
  disposeKeyboardShortcuts();
});
</script>

<template>
  <div class="app-root">
    <LoadingState v-if="appLoading" :message="appLoadingMessage" />
    <MainLayout v-else />
    <Toast />
    <ConfirmDialog />
  </div>
</template>

<style scoped>
.app-root { height: 100%; width: 100%; overflow: hidden; }
</style>
