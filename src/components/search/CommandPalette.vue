<script setup lang="ts">
/**
 * Ctrl+K command palette (RD-M9-031~034).
 *
 * Fuzzy search across the command registry and task titles, keyboard-first:
 * ArrowUp/ArrowDown move, Enter runs, Escape closes. Selecting a task jumps to
 * it in the tree (expands ancestors, selects, scrolls into view).
 */
import { computed, nextTick, ref, watch } from "vue";
import type { TaskSummary } from "../../ipc/types";
import { executeCommand, getAllCommands } from "../../app/commands";
import type { AppCommand } from "../../app/commands";
import { fuzzyRank } from "../../app/fuzzy";
import { allTasks, jumpToTask } from "../../stores/task-store";
import { commandPaletteOpen, closeCommandPalette } from "../../stores/ui-store";

type PaletteItem =
  | { kind: "command"; id: string; command: AppCommand; hint: string }
  | { kind: "task"; id: string; task: TaskSummary; hint: string };

const COMMAND_LIMIT = 8;
const TASK_LIMIT = 20;

const query = ref("");
const activeIndex = ref(0);
const inputEl = ref<HTMLInputElement | null>(null);
const listEl = ref<HTMLElement | null>(null);

const items = computed<PaletteItem[]>(() => {
  const commands = getAllCommands();
  const tasks = allTasks.value;
  const q = query.value.trim();

  if (!q) {
    return commands
      .filter((c) => c.enabled())
      .slice(0, COMMAND_LIMIT + TASK_LIMIT)
      .map((c) => commandItem(c));
  }

  const rankedCommands = fuzzyRank(
    commands,
    q,
    (c) => c.label,
    (c) => `${c.id} ${c.category} ${c.description ?? ""}`,
    COMMAND_LIMIT,
  ).map((entry) => commandItem(entry.item));

  const rankedTasks = fuzzyRank(
    tasks,
    q,
    (t) => t.title,
    (t) => `${t.task_key} ${t.status}`,
    TASK_LIMIT,
  ).map((entry): PaletteItem => ({
    kind: "task",
    id: `task:${entry.item.task_key}`,
    task: entry.item,
    hint: entry.item.status || "Not Started",
  }));

  return [...rankedCommands, ...rankedTasks];
});

function commandItem(command: AppCommand): PaletteItem {
  return {
    kind: "command",
    id: `cmd:${command.id}`,
    command,
    hint: command.shortcut ?? command.category,
  };
}

watch(query, () => {
  activeIndex.value = 0;
});

watch(commandPaletteOpen, async (open) => {
  if (!open) return;
  query.value = "";
  activeIndex.value = 0;
  await nextTick();
  inputEl.value?.focus();
  inputEl.value?.select();
});

function move(delta: number): void {
  const count = items.value.length;
  if (count === 0) {
    activeIndex.value = 0;
    return;
  }
  activeIndex.value = (activeIndex.value + delta + count) % count;
  scrollActiveIntoView();
}

function scrollActiveIntoView(): void {
  void nextTick(() => {
    const list = listEl.value;
    if (!list) return;
    const active = list.querySelector<HTMLElement>("[data-active='true']");
    active?.scrollIntoView({ block: "nearest" });
  });
}

function run(item: PaletteItem | undefined): void {
  if (!item) return;
  closeCommandPalette();
  if (item.kind === "command") {
    executeCommand(item.command.id);
    return;
  }
  if (!jumpToTask(item.task.task_key)) {
    // Task lives in a document that is not indexed yet.
    executeCommand("workspace.rebuildIndex");
  }
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
    case "PageDown":
      e.preventDefault();
      move(5);
      break;
    case "PageUp":
      e.preventDefault();
      move(-5);
      break;
    case "Home":
      e.preventDefault();
      activeIndex.value = 0;
      scrollActiveIntoView();
      break;
    case "End":
      e.preventDefault();
      activeIndex.value = Math.max(0, items.value.length - 1);
      scrollActiveIntoView();
      break;
    case "Enter":
      e.preventDefault();
      run(items.value[activeIndex.value]);
      break;
    case "Escape":
      e.preventDefault();
      closeCommandPalette();
      break;
    default:
      break;
  }
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="commandPaletteOpen"
      class="palette-overlay"
      role="presentation"
      @click.self="closeCommandPalette"
    >
      <div class="palette" role="dialog" aria-modal="true" aria-label="Command palette">
        <div class="palette__input-row">
          <i class="fas fa-terminal palette__icon"></i>
          <input
            ref="inputEl"
            v-model="query"
            class="palette__input"
            type="text"
            placeholder="Search commands and tasks… (↑↓ navigate, ↵ run, esc close)"
            spellcheck="false"
            role="combobox"
            aria-expanded="true"
            aria-controls="palette-list"
            @keydown="onKeydown"
          />
          <span class="palette__count">{{ items.length }}</span>
        </div>

        <div id="palette-list" ref="listEl" class="palette__list" role="listbox">
          <div v-if="items.length === 0" class="palette__empty">
            No commands or tasks match “{{ query }}”
          </div>
          <button
            v-for="(item, index) in items"
            :key="item.id"
            class="palette__item"
            :class="{ 'palette__item--active': index === activeIndex }"
            :data-active="index === activeIndex"
            role="option"
            :aria-selected="index === activeIndex"
            :disabled="item.kind === 'command' && !item.command.enabled()"
            @mouseenter="activeIndex = index"
            @click="run(item)"
          >
            <i
              class="palette__item-icon fas"
              :class="[
                item.kind === 'command'
                  ? (item.command.icon ?? 'fa-bolt')
                  : 'fa-list-check',
                item.kind === 'command' && !item.command.enabled() ? 'palette__item-icon--off' : '',
              ]"
            ></i>
            <span class="palette__item-label">{{
              item.kind === "command" ? item.command.label : item.task.title || "(Untitled)"
            }}</span>
            <span v-if="item.kind === 'task'" class="palette__item-key">{{ item.task.task_key }}</span>
            <span class="palette__item-hint">{{ item.hint }}</span>
          </button>
        </div>

        <div class="palette__footer">
          <span><kbd>↑</kbd><kbd>↓</kbd> navigate</span>
          <span><kbd>↵</kbd> run</span>
          <span><kbd>esc</kbd> close</span>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.palette-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.35);
  display: flex;
  align-items: flex-start;
  justify-content: center;
  padding-top: 12vh;
  z-index: var(--z-modal);
}
.palette {
  width: min(640px, 92%);
  background: var(--color-bg-primary);
  border-radius: var(--radius-xl);
  box-shadow: var(--shadow-lg);
  overflow: hidden;
  display: flex;
  flex-direction: column;
}
.palette__input-row {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-3) var(--space-4);
  border-bottom: 1px solid var(--color-border);
}
.palette__icon { color: var(--color-text-muted); }
.palette__input {
  flex: 1;
  border: none;
  outline: none;
  background: transparent;
  font-size: var(--text-base);
  color: var(--color-text-primary);
  min-width: 0;
}
.palette__count {
  font-size: var(--text-xs);
  color: var(--color-text-muted);
  background: var(--color-bg-tertiary);
  border-radius: var(--radius-full);
  padding: 0 var(--space-2);
}
.palette__list {
  max-height: 52vh;
  overflow-y: auto;
  padding: var(--space-2);
}
.palette__empty {
  padding: var(--space-6);
  text-align: center;
  color: var(--color-text-muted);
  font-size: var(--text-sm);
}
.palette__item {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  width: 100%;
  padding: var(--space-2) var(--space-3);
  border: none;
  border-radius: var(--radius-md);
  background: transparent;
  color: var(--color-text-primary);
  font-size: var(--text-sm);
  text-align: left;
  cursor: pointer;
}
.palette__item:disabled {
  color: var(--color-text-disabled);
  cursor: not-allowed;
}
.palette__item--active {
  background: var(--color-bg-selected);
}
.palette__item-icon {
  width: 16px;
  text-align: center;
  color: var(--color-text-secondary);
  flex-shrink: 0;
}
.palette__item-icon--off { color: var(--color-text-disabled); }
.palette__item-label {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.palette__item-key {
  font-family: var(--font-mono);
  font-size: var(--text-xs);
  color: var(--color-text-muted);
}
.palette__item-hint {
  font-size: var(--text-xs);
  color: var(--color-text-muted);
  flex-shrink: 0;
  max-width: 140px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.palette__footer {
  display: flex;
  gap: var(--space-4);
  padding: var(--space-2) var(--space-4);
  border-top: 1px solid var(--color-border);
  background: var(--color-bg-secondary);
  font-size: var(--text-xs);
  color: var(--color-text-muted);
}
.palette__footer kbd {
  font-family: var(--font-mono);
  background: var(--color-bg-tertiary);
  border-radius: var(--radius-sm);
  padding: 0 var(--space-1);
  margin-right: 2px;
}
</style>
