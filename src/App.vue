<script setup lang="ts">
import { onMounted } from "vue";
import MainLayout from "./components/layout/MainLayout.vue";
import Toast from "./components/common/Toast.vue";
import ConfirmDialog from "./components/common/ConfirmDialog.vue";
import LoadingState from "./components/common/LoadingState.vue";
import { appLoading, appLoadingMessage } from "./stores/app-state";
import { ping, getRuntimeInfo } from "./ipc/client";
import { initKeyboardShortcuts } from "./app/shortcuts";

onMounted(async () => {
  initKeyboardShortcuts();
  try {
    await ping();
    await getRuntimeInfo();
  } catch (e) {
    console.error("IPC not available:", e);
  }
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
