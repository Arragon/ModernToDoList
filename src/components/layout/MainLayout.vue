<script setup lang="ts">
import Sidebar from "./Sidebar.vue";
import TaskTreePanel from "./TaskTreePanel.vue";
import InspectorPanel from "./InspectorPanel.vue";
</script>

<template>
  <div class="main-layout">
    <Sidebar />
    <div class="resize-handle" />
    <TaskTreePanel />
    <div class="resize-handle" />
    <InspectorPanel />
  </div>
</template>

<style scoped>
.main-layout {
  display: flex;
  height: 100%;
  width: 100%;
  overflow: hidden;
}
</style>
