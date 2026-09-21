<script setup lang="ts">
/**
 * Virtual scroll container (RD-M10-023).
 *
 * Renders only the rows inside the viewport plus an overscan buffer. Supports
 * both fixed (`:dynamic="false"`) and variable (`:dynamic="true"`) row heights:
 * measured heights override the estimate as rows mount, and a ResizeObserver
 * keeps the cache honest when a row reflows (wrapped titles, badges, …).
 *
 * No third-party dependency — the offset table is a prefix-sum array with a
 * binary search for the first visible index, so 100k rows stay O(log n) per
 * scroll event.
 *
 * Render timing is measured through the Performance API (`app/perf.ts`) and is
 * the evidence the RD-M10-022 decision gate reads via `window.__mtodoPerf`.
 */
import {
  computed, nextTick, onBeforeUnmount, onBeforeUpdate, onMounted, onUpdated, ref, watch,
} from "vue";
import { beginRender, endRender } from "../../app/perf";
import type { RenderTimer } from "../../app/perf";

const props = withDefaults(defineProps<{
  itemCount: number;
  /** Estimated (or exact, when `dynamic` is false) row height in px. */
  itemHeight?: number;
  /** Extra rows rendered above/below the viewport. */
  overscan?: number;
  /** Measure real heights instead of trusting `itemHeight`. */
  dynamic?: boolean;
  ariaLabel?: string;
}>(), {
  itemHeight: 32,
  overscan: 8,
  dynamic: true,
  ariaLabel: "Virtual list",
});

const scrollerEl = ref<HTMLElement | null>(null);
const contentEl = ref<HTMLElement | null>(null);
const scrollTop = ref(0);
const viewportHeight = ref(0);

/** index -> measured height (only for rows that differ from the estimate). */
const measured = ref<Map<number, number>>(new Map());

let renderTimer: RenderTimer | null = null;
let measureScheduled = false;
let resizeObserver: ResizeObserver | null = null;

function heightFor(index: number): number {
  if (!props.dynamic) return props.itemHeight;
  return measured.value.get(index) ?? props.itemHeight;
}

/** Prefix sums: offsets[i] = top edge of row i; offsets[itemCount] = total. */
const offsets = computed<number[]>(() => {
  const count = Math.max(0, props.itemCount);
  const out = new Array<number>(count + 1);
  out[0] = 0;
  for (let i = 0; i < count; i += 1) {
    out[i + 1] = out[i] + heightFor(i);
  }
  return out;
});

const totalHeight = computed(() => {
  const table = offsets.value;
  return table.length > 0 ? table[table.length - 1] : 0;
});

/** First row index whose bottom edge is below `top`. */
function lowerBound(top: number): number {
  const table = offsets.value;
  let low = 0;
  let high = Math.max(0, table.length - 2);
  while (low < high) {
    const mid = (low + high) >> 1;
    if (table[mid + 1] <= top) low = mid + 1;
    else high = mid;
  }
  return low;
}

const startIndex = computed(() => {
  if (props.itemCount === 0) return 0;
  return Math.max(0, lowerBound(scrollTop.value) - props.overscan);
});

const endIndex = computed(() => {
  if (props.itemCount === 0) return 0;
  const bottom = scrollTop.value + Math.max(viewportHeight.value, 1);
  const last = lowerBound(bottom) + props.overscan;
  return Math.min(props.itemCount - 1, last);
});

/** Indices handed to the slot, in order. */
const visibleIndices = computed<number[]>(() => {
  const out: number[] = [];
  for (let i = startIndex.value; i <= endIndex.value && i < props.itemCount; i += 1) {
    out.push(i);
  }
  return out;
});

const offsetTop = computed(() => offsets.value[startIndex.value] ?? 0);
const renderedCount = computed(() => visibleIndices.value.length);

function onScroll(): void {
  const el = scrollerEl.value;
  if (!el) return;
  scrollTop.value = el.scrollTop;
}

function measureVisibleRows(): void {
  measureScheduled = false;
  if (!props.dynamic) return;
  const content = contentEl.value;
  if (!content) return;

  const children = content.children;
  const start = startIndex.value;
  let changed = false;
  const next = new Map(measured.value);

  for (let i = 0; i < children.length && start + i < props.itemCount; i += 1) {
    const el = children[i] as HTMLElement;
    const height = el.offsetHeight;
    if (height <= 0) continue;
    const index = start + i;
    if (next.get(index) !== height) {
      next.set(index, height);
      changed = true;
    }
  }

  // Drop measurements for rows that no longer exist.
  if (next.size > props.itemCount) {
    for (const key of Array.from(next.keys())) {
      if (key >= props.itemCount) {
        next.delete(key);
        changed = true;
      }
    }
  }

  if (changed) measured.value = next;
}

function scheduleMeasure(): void {
  if (measureScheduled) return;
  measureScheduled = true;
  requestAnimationFrame(() => {
    void nextTick(measureVisibleRows);
  });
}

/** Forgets every measurement (e.g. after the row set changes wholesale). */
function remeasure(): void {
  measured.value = new Map();
  scheduleMeasure();
}

function scrollToIndex(index: number, align: "start" | "center" | "nearest" = "nearest"): void {
  const el = scrollerEl.value;
  if (!el || index < 0 || index >= props.itemCount) return;
  const table = offsets.value;
  const top = table[index] ?? 0;
  const height = heightFor(index);
  const view = el.clientHeight;

  let target = top;
  if (align === "center") {
    target = top - Math.max(0, (view - height) / 2);
  } else if (align === "nearest") {
    if (top >= el.scrollTop && top + height <= el.scrollTop + view) return;
    target = top < el.scrollTop ? top : top - view + height;
  }

  el.scrollTop = Math.max(0, Math.min(target, totalHeight.value - view));
  scrollTop.value = el.scrollTop;
}

function resetScroll(): void {
  const el = scrollerEl.value;
  if (!el) return;
  el.scrollTop = 0;
  scrollTop.value = 0;
}

onBeforeUpdate(() => {
  renderTimer = beginRender("task-tree:render");
});

onUpdated(() => {
  if (renderTimer) {
    endRender(renderTimer, {
      rowCount: props.itemCount,
      renderedCount: renderedCount.value,
      virtualized: true,
    });
    renderTimer = null;
  }
  scheduleMeasure();
});

function updateViewport(): void {
  const el = scrollerEl.value;
  if (!el) return;
  viewportHeight.value = el.clientHeight;
}

onMounted(() => {
  updateViewport();
  if (typeof ResizeObserver !== "undefined") {
    resizeObserver = new ResizeObserver(() => {
      updateViewport();
      scheduleMeasure();
    });
    if (scrollerEl.value) resizeObserver.observe(scrollerEl.value);
    if (contentEl.value) resizeObserver.observe(contentEl.value);
  }
  renderTimer = beginRender("task-tree:mount");
  void nextTick(() => {
    measureVisibleRows();
    if (renderTimer) {
      endRender(renderTimer, {
        rowCount: props.itemCount,
        renderedCount: renderedCount.value,
        virtualized: true,
      });
      renderTimer = null;
    }
  });
});

onBeforeUnmount(() => {
  if (resizeObserver) {
    resizeObserver.disconnect();
    resizeObserver = null;
  }
  measureScheduled = false;
  renderTimer = null;
});

// A wholesale row-set change invalidates cached heights.
watch(() => props.itemCount, () => {
  scheduleMeasure();
});

defineExpose({ scrollToIndex, remeasure, resetScroll });
</script>

<template>
  <div
    ref="scrollerEl"
    class="virtual-list"
    role="presentation"
    :aria-label="ariaLabel"
    @scroll.passive="onScroll"
  >
    <div class="virtual-list__spacer" :style="{ height: `${totalHeight}px` }">
      <div
        ref="contentEl"
        class="virtual-list__content"
        :style="{ transform: `translateY(${offsetTop}px)` }"
        role="group"
      >
        <div
          v-for="index in visibleIndices"
          :key="index"
          class="virtual-list__row"
          :data-index="index"
        >
          <slot name="item" :index="index" />
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.virtual-list {
  height: 100%;
  overflow-y: auto;
  overflow-x: hidden;
  contain: layout paint;
}
.virtual-list__spacer {
  position: relative;
  width: 100%;
}
.virtual-list__content {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  will-change: transform;
}
.virtual-list__row {
  width: 100%;
}
</style>
