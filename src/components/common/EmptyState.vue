<script setup lang="ts">
defineProps<{
  icon?: string;
  title: string;
  description?: string;
}>();
</script>

<template>
  <div class="empty-state">
    <i v-if="icon" :class="['fas', icon, 'empty-state__icon']"></i>
    <h3 class="empty-state__title">{{ title }}</h3>
    <p v-if="description" class="empty-state__desc">{{ description }}</p>
    <slot />
  </div>
</template>

<style scoped>
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: var(--space-12);
  text-align: center;
  color: var(--color-text-muted);
}
.empty-state__icon {
  font-size: 3rem;
  margin-bottom: var(--space-4);
  opacity: 0.4;
}
.empty-state__title {
  font-size: var(--text-lg);
  font-weight: 600;
  color: var(--color-text-secondary);
  margin-bottom: var(--space-2);
}
.empty-state__desc {
  font-size: var(--text-sm);
  max-width: 300px;
}
</style>
