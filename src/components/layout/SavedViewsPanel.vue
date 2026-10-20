<script setup lang="ts">
/**
 * Saved Views sidebar section (RD-M9-028~030).
 *
 * Lists each saved view with an icon and a live result count, and provides
 * create (from the current filter state), rename and delete. Clicking a view
 * applies its predicates to the active filter.
 */
import { computed, onMounted } from "vue";
import {
  activeViewId, applyView, canUpdateView, countFor, createViewFromFilter,
  loadSavedViews, moveView, removeView, renameView, savedViews,
  updateViewFromFilter, usingBackendStore,
} from "../../stores/saved-view-store";
import { describePredicates } from "../../app/view-predicates";
import { clearFilters, isFilterActive } from "../../stores/filter-state";
import { promptForText, showConfirm, showToast } from "../../stores/app-state";

const views = computed(() => savedViews.value);
const canSave = computed(() => isFilterActive.value);

onMounted(() => {
  void loadSavedViews();
});

async function create(): Promise<void> {
  const name = await promptForText({
    title: "Save View",
    message: "Name this view. It captures the active filter conditions.",
    placeholder: "Open high-priority work",
  });
  if (!name) return;
  await createViewFromFilter(name);
}

async function rename(id: string, current: string): Promise<void> {
  const name = await promptForText({
    title: "Rename View",
    defaultValue: current,
    confirmLabel: "Rename",
  });
  if (!name || name === current) return;
  const ok = await renameView(id, name);
  if (ok) showToast("View renamed", "success");
}

/** Re-saves the current filter conditions into the view (RD-M9-028~030). */
async function resave(id: string, name: string): Promise<void> {
  const ok = await updateViewFromFilter(id);
  if (ok) showToast(`View "${name}" updated with the current filter`, "success");
}

async function reorder(id: string, direction: -1 | 1): Promise<void> {
  await moveView(id, direction);
}

function confirmDelete(id: string, name: string): void {
  showConfirm(
    "Delete saved view?",
    `“${name}” will be removed. Your tasks are not affected.`,
    () => { void removeView(id); },
    "Delete",
  );
}

function select(id: string): void {
  if (activeViewId.value === id) {
    clearFilters();
    activeViewId.value = null;
    return;
  }
  applyView(id);
}
</script>

<template>
  <div class="sidebar__section saved-views">
    <div class="sidebar__section-title saved-views__title">
      <span>Saved Views</span>
      <button
        class="saved-views__add"
        :disabled="!canSave"
        title="Save the current filter as a view"
        @click="create"
      >
        <i class="fas fa-plus"></i>
      </button>
    </div>

    <div v-if="views.length === 0" class="sidebar__hint">
      No saved views. Apply a filter, then click +.
    </div>

    <div
      v-for="(view, index) in views"
      :key="view.id"
      class="saved-views__item"
      :class="{ 'saved-views__item--active': activeViewId === view.id }"
      :title="describePredicates(view.predicates)"
      role="button"
      tabindex="0"
      @click="select(view.id)"
      @keydown.enter.prevent="select(view.id)"
      @keydown.space.prevent="select(view.id)"
    >
      <i class="fas fa-bookmark saved-views__icon"></i>
      <span class="saved-views__name truncate-text">{{ view.name }}</span>
      <span class="saved-views__count">{{ countFor(view.id) }}</span>
      <span class="saved-views__actions">
        <button
          v-if="activeViewId === view.id && canUpdateView(view.id)"
          class="saved-views__action"
          title="Re-save the current filter into this view"
          @click.stop="resave(view.id, view.name)"
        >
          <i class="fas fa-floppy-disk"></i>
        </button>
        <button
          class="saved-views__action"
          :class="{ 'saved-views__action--disabled': index === 0 }"
          :disabled="index === 0"
          title="Move up"
          @click.stop="reorder(view.id, -1)"
        >
          <i class="fas fa-chevron-up"></i>
        </button>
        <button
          class="saved-views__action"
          :class="{ 'saved-views__action--disabled': index === views.length - 1 }"
          :disabled="index === views.length - 1"
          title="Move down"
          @click.stop="reorder(view.id, 1)"
        >
          <i class="fas fa-chevron-down"></i>
        </button>
        <button
          class="saved-views__action"
          title="Rename"
          @click.stop="rename(view.id, view.name)"
        >
          <i class="fas fa-pen"></i>
        </button>
        <button
          class="saved-views__action saved-views__action--danger"
          title="Delete"
          @click.stop="confirmDelete(view.id, view.name)"
        >
          <i class="fas fa-trash"></i>
        </button>
      </span>
    </div>

    <div v-if="views.length > 0 && !usingBackendStore" class="saved-views__note">
      <i class="fas fa-circle-info"></i> stored locally (backend views command unavailable)
    </div>
  </div>
</template>

<style scoped>
.saved-views__title {
  display: flex;
  align-items: center;
  justify-content: space-between;
}
.saved-views__add {
  background: none;
  border: none;
  cursor: pointer;
  color: var(--color-text-muted);
  font-size: var(--text-xs);
  padding: 0 var(--space-1);
}
.saved-views__add:hover:not(:disabled) { color: var(--color-accent); }
.saved-views__add:disabled { color: var(--color-text-disabled); cursor: not-allowed; }
.saved-views__item {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-1) var(--space-2);
  border-radius: var(--radius-sm);
  font-size: var(--text-sm);
  color: var(--color-text-secondary);
  cursor: pointer;
  min-width: 0;
}
.saved-views__item:hover { background: var(--color-bg-hover); }
.saved-views__item--active {
  background: var(--color-bg-selected);
  color: var(--color-text-primary);
  font-weight: 600;
}
.saved-views__icon {
  font-size: var(--text-xs);
  color: var(--color-accent);
  flex-shrink: 0;
}
.saved-views__name { flex: 1; min-width: 0; }
.saved-views__count {
  font-size: var(--text-xs);
  color: var(--color-text-muted);
  background: var(--color-bg-tertiary);
  border-radius: var(--radius-full);
  padding: 0 var(--space-2);
  flex-shrink: 0;
}
.saved-views__actions {
  display: none;
  gap: 2px;
  flex-shrink: 0;
}
.saved-views__item:hover .saved-views__actions { display: inline-flex; }
.saved-views__action {
  background: none;
  border: none;
  cursor: pointer;
  color: var(--color-text-muted);
  font-size: var(--text-xs);
  padding: 0 2px;
}
.saved-views__action:hover { color: var(--color-text-primary); }
.saved-views__action--danger:hover { color: var(--color-error); }
.saved-views__action--disabled,
.saved-views__action:disabled {
  opacity: 0.35;
  cursor: not-allowed;
}
.saved-views__action--disabled:hover,
.saved-views__action:disabled:hover { color: var(--color-text-muted); }
.saved-views__note {
  margin-top: var(--space-2);
  font-size: var(--text-xs);
  color: var(--color-text-muted);
  display: flex;
  align-items: center;
  gap: var(--space-1);
}
</style>
