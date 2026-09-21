<script setup lang="ts">
defineProps<{
  message?: string;
}>();
</script>

<template>
  <div class="loading-state">
    <div class="loading-state__spinner">
      <i class="fas fa-spinner fa-spin"></i>
    </div>
    <p v-if="message" class="loading-state__text">{{ message }}</p>
  </div>
</template>

<style scoped>
.loading-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: var(--space-12);
  gap: var(--space-4);
}
.loading-state__spinner {
  font-size: 2rem;
  color: var(--color-accent);
}
.loading-state__text {
  font-size: var(--text-sm);
  color: var(--color-text-muted);
}
</style>
