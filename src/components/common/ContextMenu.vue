<script setup lang="ts">
/**
 * Generic right-click context menu — the codebase's first one (RD-M8 transfer
 * entry points). Fixed-position, closes on outside click / Escape / scroll.
 */
import { onBeforeUnmount, onMounted, ref, watch } from "vue";

export interface ContextMenuItem {
  id: string;
  label: string;
  icon?: string;
  danger?: boolean;
  disabled?: boolean;
}

const props = defineProps<{
  visible: boolean;
  x: number;
  y: number;
  items: ContextMenuItem[];
}>();

const emit = defineEmits<{
  select: [id: string];
  close: [];
}>();

const menuEl = ref<HTMLElement | null>(null);

function onSelect(item: ContextMenuItem): void {
  if (item.disabled) return;
  emit("select", item.id);
  emit("close");
}

function onPointerDown(e: PointerEvent): void {
  if (!props.visible) return;
  if (menuEl.value && e.target instanceof Node && menuEl.value.contains(e.target)) return;
  emit("close");
}

function onKeyDown(e: KeyboardEvent): void {
  if (props.visible && e.key === "Escape") emit("close");
}

onMounted(() => {
  window.addEventListener("pointerdown", onPointerDown, true);
  window.addEventListener("keydown", onKeyDown);
  window.addEventListener("resize", () => emit("close"));
});
onBeforeUnmount(() => {
  window.removeEventListener("pointerdown", onPointerDown, true);
  window.removeEventListener("keydown", onKeyDown);
});

// Keep the menu inside the viewport when opened near an edge.
const posX = ref(0);
const posY = ref(0);
watch(() => [props.visible, props.x, props.y], () => {
  if (!props.visible) return;
  const width = 220;
  const height = props.items.length * 32 + 8;
  posX.value = Math.min(props.x, window.innerWidth - width - 8);
  posY.value = Math.min(props.y, window.innerHeight - height - 8);
}, { immediate: true });
</script>

<template>
  <Teleport to="body">
    <div
      v-if="visible"
      ref="menuEl"
      class="context-menu"
      :style="{ left: `${posX}px`, top: `${posY}px` }"
      role="menu"
    >
      <button
        v-for="item in items"
        :key="item.id"
        class="context-menu__item"
        :class="{ 'context-menu__item--danger': item.danger }"
        :disabled="item.disabled"
        role="menuitem"
        @click="onSelect(item)"
      >
        <i v-if="item.icon" class="fas" :class="item.icon"></i>
        <span>{{ item.label }}</span>
      </button>
    </div>
  </Teleport>
</template>

<style scoped>
.context-menu {
  position: fixed;
  z-index: var(--z-modal);
  min-width: 200px;
  background: var(--color-bg-primary);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-lg);
  padding: var(--space-1) 0;
}
.context-menu__item {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  width: 100%;
  padding: var(--space-1) var(--space-3);
  background: none;
  border: none;
  cursor: pointer;
  font-size: var(--text-sm);
  color: var(--color-text-primary);
  text-align: left;
  min-height: 32px;
}
.context-menu__item:hover:not(:disabled) {
  background: var(--color-bg-hover);
}
.context-menu__item:disabled {
  color: var(--color-text-disabled);
  cursor: not-allowed;
}
.context-menu__item--danger {
  color: var(--color-error);
}
.context-menu__item i {
  font-size: var(--text-xs);
  width: 14px;
  text-align: center;
}
</style>
