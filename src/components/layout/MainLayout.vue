<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";
import Sidebar from "./Sidebar.vue";
import TaskTreePanel from "./TaskTreePanel.vue";
import InspectorPanel from "./InspectorPanel.vue";
import CommandPalette from "../search/CommandPalette.vue";
import PathInputDialog from "../common/PathInputDialog.vue";
import TextPromptDialog from "../common/TextPromptDialog.vue";
import { inspectorWidth, layoutLimits, sidebarWidth } from "../../stores/settings-store";

type HandleKind = "sidebar" | "inspector";

const rootEl = ref<HTMLElement | null>(null);
const activeHandle = ref<HandleKind | null>(null);
const limits = layoutLimits();

interface DragState {
  kind: HandleKind;
  pointerId: number;
  startX: number;
  startWidth: number;
  element: HTMLElement;
}

let drag: DragState | null = null;

/** Applies the token-derived CSS variables that both panels read for width. */
function applyWidths(): void {
  const el = rootEl.value;
  if (!el) return;
  el.style.setProperty("--sidebar-width", `${Math.round(sidebarWidth.value)}px`);
  el.style.setProperty("--inspector-width", `${Math.round(inspectorWidth.value)}px`);
}

function onPointerDown(kind: HandleKind, event: PointerEvent): void {
  if (event.button !== 0) return;
  const element = event.currentTarget as HTMLElement | null;
  if (!element) return;

  event.preventDefault();
  drag = {
    kind,
    pointerId: event.pointerId,
    startX: event.clientX,
    startWidth: kind === "sidebar" ? sidebarWidth.value : inspectorWidth.value,
    element,
  };
  activeHandle.value = kind;

  // Pointer capture keeps the drag alive even if the cursor leaves the handle,
  // and guarantees we receive the matching pointerup/pointercancel.
  try {
    element.setPointerCapture(event.pointerId);
  } catch {
    // Older WebViews fall back to window listeners installed below.
    window.addEventListener("pointermove", onWindowPointerMove);
    window.addEventListener("pointerup", onWindowPointerUp);
  }
  document.body.classList.add("is-resizing");
}

function onPointerMove(event: PointerEvent): void {
  if (!drag || event.pointerId !== drag.pointerId) return;
  const delta = event.clientX - drag.startX;
  if (drag.kind === "sidebar") {
    // Dragging right widens the sidebar.
    sidebarWidth.value = clamp(drag.startWidth + delta, limits.sidebarMin, limits.sidebarMax);
  } else {
    // The inspector sits on the right: dragging left widens it.
    inspectorWidth.value = clamp(drag.startWidth - delta, limits.inspectorMin, limits.inspectorMax);
  }
  applyWidths();
}

function onPointerUp(event: PointerEvent): void {
  if (!drag || event.pointerId !== drag.pointerId) return;
  endDrag();
}

function onLostPointerCapture(): void {
  if (drag) endDrag();
}

function onWindowPointerMove(event: PointerEvent): void {
  onPointerMove(event);
}

function onWindowPointerUp(event: PointerEvent): void {
  onPointerUp(event);
}

function endDrag(): void {
  const element = drag?.element;
  const pointerId = drag?.pointerId;
  drag = null;
  activeHandle.value = null;
  window.removeEventListener("pointermove", onWindowPointerMove);
  window.removeEventListener("pointerup", onWindowPointerUp);
  if (element && pointerId !== undefined) {
    try {
      if (element.hasPointerCapture(pointerId)) element.releasePointerCapture(pointerId);
    } catch {
      // Capture was already released; nothing to clean up.
    }
  }
  document.body.classList.remove("is-resizing");
}

/** Double-click restores the token default width. */
function onDoubleClick(kind: HandleKind): void {
  if (kind === "sidebar") sidebarWidth.value = limits.sidebarMin + 60;
  else inspectorWidth.value = limits.inspectorMin + 80;
  applyWidths();
}

/** Keyboard resizing keeps the handles accessible without a pointer. */
function nudge(kind: HandleKind, direction: -1 | 1): void {
  const step = 16 * direction;
  if (kind === "sidebar") {
    sidebarWidth.value = clamp(sidebarWidth.value + step, limits.sidebarMin, limits.sidebarMax);
  } else {
    inspectorWidth.value = clamp(inspectorWidth.value - step, limits.inspectorMin, limits.inspectorMax);
  }
  applyWidths();
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

onMounted(() => {
  applyWidths();
});

onBeforeUnmount(() => {
  // No listener may outlive the component.
  window.removeEventListener("pointermove", onWindowPointerMove);
  window.removeEventListener("pointerup", onWindowPointerUp);
  document.body.classList.remove("is-resizing");
  drag = null;
  activeHandle.value = null;
});
</script>

<template>
  <div ref="rootEl" class="main-layout">
    <Sidebar />
    <div
      class="resize-handle"
      :class="{ active: activeHandle === 'sidebar' }"
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize sidebar"
      tabindex="0"
      @pointerdown="onPointerDown('sidebar', $event)"
      @pointermove="onPointerMove"
      @pointerup="onPointerUp"
      @pointercancel="onPointerUp"
      @lostpointercapture="onLostPointerCapture"
      @dblclick="onDoubleClick('sidebar')"
      @keydown.left="nudge('sidebar', -1)"
      @keydown.right="nudge('sidebar', 1)"
    />
    <TaskTreePanel />
    <div
      class="resize-handle"
      :class="{ active: activeHandle === 'inspector' }"
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize inspector"
      tabindex="0"
      @pointerdown="onPointerDown('inspector', $event)"
      @pointermove="onPointerMove"
      @pointerup="onPointerUp"
      @pointercancel="onPointerUp"
      @lostpointercapture="onLostPointerCapture"
      @dblclick="onDoubleClick('inspector')"
      @keydown.left="nudge('inspector', 1)"
      @keydown.right="nudge('inspector', -1)"
    />
    <InspectorPanel />

    <CommandPalette />
    <PathInputDialog />
    <TextPromptDialog />
  </div>
</template>

<style scoped>
.main-layout {
  display: flex;
  height: 100%;
  width: 100%;
  overflow: hidden;
}
.resize-handle {
  touch-action: none;
}
</style>

<style>
/* Global: block text selection and force the resize cursor while dragging. */
body.is-resizing {
  cursor: col-resize;
  user-select: none;
}
body.is-resizing iframe,
body.is-resizing webview {
  pointer-events: none;
}
</style>
