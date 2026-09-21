<script setup lang="ts">
import Inspector from "../inspector/Inspector.vue";
</script>

<template>
  <div class="inspector-panel">
    <div class="inspector-panel__header">
      <h3>Inspector</h3>
    </div>
    <div class="inspector-panel__content">
      <Inspector />
    </div>
  </div>
</template>

<style scoped>
.inspector-panel {
  display: flex;
  flex-direction: column;
  width: var(--inspector-width);
  min-width: var(--inspector-min-width);
  background: var(--color-bg-secondary);
  overflow: hidden;
}
.inspector-panel__header {
  display: flex;
  align-items: center;
  padding: var(--space-2) var(--space-4);
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
  height: var(--toolbar-height);
}
.inspector-panel__header h3 { font-size: var(--text-sm); font-weight: 600; }
.inspector-panel__content { flex: 1; overflow-y: auto; }
</style>
