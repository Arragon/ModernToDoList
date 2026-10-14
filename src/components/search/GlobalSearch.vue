<script setup lang="ts">
/**
 * Global Search (RD-M9-008~013).
 *
 * Debounced query → `global_search` (FTS5 + CJK LIKE fallback server-side).
 * When that command is not registered yet the store falls back to an in-memory
 * fuzzy search over the loaded index, and the source is shown in the footer.
 * Keyboard: ↑/↓ move, Enter jumps to the task in the tree, Esc closes.
 */
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";
import {
  globalSearchOpen, closeGlobalSearch,
} from "../../stores/ui-store";
import {
  clearSearch, runSearch, searchHits, searching, searchSource, searchTotal, searchTruncated, jumpToHit,
} from "../../stores/search-store";
import type { SearchHit } from "../../stores/search-store";

const DEBOUNCE_MS = 200;
const RESULT_LIMIT = 50;

const query = ref("");
const activeIndex = ref(0);
const inputEl = ref<HTMLInputElement | null>(null);
const listEl = ref<HTMLElement | null>(null);

let debounce: ReturnType<typeof setTimeout> | null = null;

const hits = computed(() => searchHits.value);
const sourceLabel = computed(() => {
  if (searchSource.value === "backend") return "index search";
  if (searchSource.value === "local") return "local fallback (backend search unavailable)";
  return "";
});

watch(query, (value) => {
  activeIndex.value = 0;
  if (debounce) clearTimeout(debounce);
  debounce = setTimeout(() => {
    void runSearch(value, RESULT_LIMIT);
  }, DEBOUNCE_MS);
});

watch(globalSearchOpen, async (open) => {
  if (!open) return;
  await nextTick();
  inputEl.value?.focus();
  inputEl.value?.select();
});

onBeforeUnmount(() => {
  if (debounce) clearTimeout(debounce);
  debounce = null;
});

function move(delta: number): void {
  const count = hits.value.length;
  if (count === 0) {
    activeIndex.value = 0;
    return;
  }
  activeIndex.value = (activeIndex.value + delta + count) % count;
  void nextTick(() => {
    listEl.value
      ?.querySelector<HTMLElement>("[data-active='true']")
      ?.scrollIntoView({ block: "nearest" });
  });
}

function jump(hit: SearchHit | undefined): void {
  if (!hit) return;
  if (jumpToHit(hit)) close();
}

function close(): void {
  closeGlobalSearch();
  clearSearch();
  query.value = "";
}

function onKeydown(e: KeyboardEvent): void {
  switch (e.key) {
    case "ArrowDown":
      e.preventDefault();
      move(1);
      break;
    case "ArrowUp":
      e.preventDefault();
      move(-1);
      break;
    case "Enter":
      e.preventDefault();
      jump(hits.value[activeIndex.value]);
      break;
    case "Escape":
      e.preventDefault();
      close();
      break;
    default:
      break;
  }
}
</script>

<template>
  <div v-if="globalSearchOpen" class="global-search" role="search">
    <div class="global-search__input-row">
      <i class="fas fa-magnifying-glass global-search__icon"></i>
      <input
        ref="inputEl"
        v-model="query"
        class="global-search__input"
        type="text"
        placeholder="Search tasks, tags, participants, attachments…"
        spellcheck="false"
        role="combobox"
        aria-expanded="true"
        aria-controls="global-search-results"
        @keydown="onKeydown"
      />
      <button class="global-search__close" title="Close search (Esc)" @click="close">
        <i class="fas fa-xmark"></i>
      </button>
    </div>

    <div id="global-search-results" ref="listEl" class="global-search__results" role="listbox">
      <div v-if="searching" class="global-search__status">
        <i class="fas fa-spinner fa-spin"></i> Searching…
      </div>
      <div v-else-if="query.trim() && hits.length === 0" class="global-search__status">
        No matches for “{{ query.trim() }}”
      </div>
      <template v-else>
        <button
          v-for="(hit, index) in hits"
          :key="`${hit.documentId}:${hit.taskKey}`"
          class="global-search__hit"
          :class="{ 'global-search__hit--active': index === activeIndex }"
          :data-active="index === activeIndex"
          role="option"
          :aria-selected="index === activeIndex"
          @mouseenter="activeIndex = index"
          @click="jump(hit)"
        >
          <span class="global-search__hit-title">{{ hit.title || "(Untitled)" }}</span>
          <span class="global-search__hit-meta">
            <span class="global-search__hit-key">{{ hit.taskKey }}</span>
            <span v-if="hit.snippet && hit.snippet !== hit.title" class="global-search__hit-snippet">
              {{ hit.snippet }}
            </span>
            <span v-if="hit.matchedField" class="chip">{{ hit.matchedField }}</span>
          </span>
        </button>
      </template>
    </div>

    <div class="global-search__footer">
      <span>{{ hits.length }} of {{ searchTotal }} result(s)</span>
      <span v-if="searchTruncated" class="global-search__truncated">truncated</span>
      <span v-if="sourceLabel" class="global-search__source">{{ sourceLabel }}</span>
      <span class="global-search__keys"><kbd>↑</kbd><kbd>↓</kbd> move · <kbd>↵</kbd> jump · <kbd>esc</kbd> close</span>
    </div>
  </div>
</template>

<style scoped>
.global-search {
  display: flex;
  flex-direction: column;
  border-bottom: 1px solid var(--color-border);
  background: var(--color-bg-secondary);
  flex-shrink: 0;
}
.global-search__input-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
}
.global-search__icon { color: var(--color-text-muted); font-size: var(--text-sm); }
.global-search__input {
  flex: 1;
  min-width: 0;
  padding: var(--space-1) var(--space-2);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: var(--color-bg-primary);
  color: var(--color-text-primary);
  font-size: var(--text-sm);
  outline: none;
}
.global-search__input:focus { border-color: var(--color-border-focus); }
.global-search__close {
  background: none;
  border: none;
  cursor: pointer;
  color: var(--color-text-muted);
  padding: var(--space-1);
}
.global-search__close:hover { color: var(--color-text-primary); }
.global-search__results {
  max-height: 40vh;
  overflow-y: auto;
  padding: 0 var(--space-2) var(--space-2);
}
.global-search__status {
  padding: var(--space-3);
  font-size: var(--text-sm);
  color: var(--color-text-muted);
  display: flex;
  align-items: center;
  gap: var(--space-2);
}
.global-search__hit {
  display: flex;
  flex-direction: column;
  gap: 2px;
  width: 100%;
  padding: var(--space-2);
  border: none;
  border-radius: var(--radius-md);
  background: transparent;
  text-align: left;
  cursor: pointer;
  color: var(--color-text-primary);
}
.global-search__hit--active { background: var(--color-bg-selected); }
.global-search__hit-title {
  font-size: var(--text-sm);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.global-search__hit-meta {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-xs);
  color: var(--color-text-muted);
  min-width: 0;
}
.global-search__hit-key {
  font-family: var(--font-mono);
  flex-shrink: 0;
}
.global-search__hit-snippet {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.global-search__footer {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-1) var(--space-3);
  border-top: 1px solid var(--color-border);
  font-size: var(--text-xs);
  color: var(--color-text-muted);
}
.global-search__truncated { color: var(--color-warning); }
.global-search__source { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.global-search__keys kbd {
  font-family: var(--font-mono);
  background: var(--color-bg-tertiary);
  border-radius: var(--radius-sm);
  padding: 0 var(--space-1);
}
</style>
