<script setup lang="ts">
/**
 * Smart Views sidebar section (RD-M9-014~021).
 *
 * Lists the built-in views with live counts. Flat views project a key set
 * into the task tree; `by_participant` reuses the tree's participant
 * grouping; `by_tag` drills down per group using the authoritative backend
 * evaluation (client-side fallback over cached tags).
 *
 * Smart views are read-only projections — selecting one never mutates task
 * data. When the backend command is unavailable the counts come from the
 * client-side mirror in `app/smart-views.ts`.
 */
import { computed } from "vue";
import { SMART_VIEWS, smartViewDef } from "../../app/smart-views";
import type { SmartViewKind } from "../../ipc/types";
import {
  clearSmartGroup, clearSmartView, expandedSmartGroup,
  selectSmartGroup, selectSmartView, smartViewCounts, smartViewGroups,
  smartViewLoading, smartViewTotal,
} from "../../stores/smart-view-store";
import { activeSmartView } from "../../stores/smart-view-state";
import { Commands } from "../../ipc/commands";
import { isCommandAvailable } from "../../stores/capability-store";

const counts = computed(() => smartViewCounts.value);
const backendAvailable = computed(() => isCommandAvailable(Commands.GET_SMART_VIEW));
const flatViews = computed(() => SMART_VIEWS.filter((v) => !v.grouping));

const activeKind = computed(() => activeSmartView.value);
const byTagActive = computed(() => activeKind.value === "by_tag");
const tagGroups = computed(() => smartViewGroups.value);
const total = computed(() => smartViewTotal.value);

function isActive(kind: string): boolean {
  return activeKind.value === kind;
}

async function toggleFlat(kind: SmartViewKind): Promise<void> {
  if (kind === "by_participant" || kind === "by_tag") return;
  await selectSmartView(kind);
}

async function toggleByParticipant(): Promise<void> {
  await selectSmartView("by_participant");
}

async function toggleByTag(): Promise<void> {
  await selectSmartView("by_tag");
}

function drillDown(name: string): void {
  if (expandedSmartGroup.value === name) {
    clearSmartGroup();
    return;
  }
  selectSmartGroup(name);
}

function dismiss(): void {
  clearSmartView();
}
</script>

<template>
  <div class="sidebar__section smart-views">
    <div class="sidebar__section-title smart-views__title">
      <span>Smart Views</span>
      <button
        v-if="activeKind"
        class="smart-views__clear"
        title="Clear the active smart view"
        @click="dismiss"
      >
        <i class="fas fa-xmark"></i>
      </button>
    </div>

    <div
      v-for="view in flatViews"
      :key="view.kind"
      class="smart-views__item"
      :class="{ 'smart-views__item--active': isActive(view.kind) }"
      :title="view.hint"
      role="button"
      tabindex="0"
      @click="toggleFlat(view.kind)"
      @keydown.enter.prevent="toggleFlat(view.kind)"
      @keydown.space.prevent="toggleFlat(view.kind)"
    >
      <i :class="['fas', view.icon, 'smart-views__icon']"></i>
      <span class="smart-views__name truncate-text">{{ view.label }}</span>
      <span class="smart-views__count">{{ counts[view.kind] }}</span>
    </div>

    <div
      class="smart-views__item"
      :class="{ 'smart-views__item--active': isActive('by_participant') }"
      :title="smartViewDef('by_participant')?.hint"
      role="button"
      tabindex="0"
      @click="toggleByParticipant"
      @keydown.enter.prevent="toggleByParticipant"
      @keydown.space.prevent="toggleByParticipant"
    >
      <i class="fas fa-users smart-views__icon"></i>
      <span class="smart-views__name truncate-text">By Participant</span>
    </div>

    <div
      class="smart-views__item"
      :class="{ 'smart-views__item--active': isActive('by_tag') }"
      :title="smartViewDef('by_tag')?.hint"
      role="button"
      tabindex="0"
      @click="toggleByTag"
      @keydown.enter.prevent="toggleByTag"
      @keydown.space.prevent="toggleByTag"
    >
      <i class="fas fa-tags smart-views__icon"></i>
      <span class="smart-views__name truncate-text">By Tag</span>
      <i v-if="byTagActive" :class="['fas', expandedSmartGroup === '__all__' ? 'fa-chevron-down' : 'fa-chevron-right', 'smart-views__chevron']"></i>
    </div>

    <div v-if="byTagActive && expandedSmartGroup === '__all__'" class="smart-views__groups">
      <div v-if="smartViewLoading" class="sidebar__hint">Evaluating…</div>
      <div v-else-if="tagGroups.length === 0" class="sidebar__hint">No tagged tasks.</div>
      <div
        v-for="group in tagGroups"
        :key="group.name"
        class="smart-views__group"
        :class="{ 'smart-views__group--active': expandedSmartGroup === group.name }"
        role="button"
        tabindex="0"
        :title="`Show only tasks tagged “${group.name}”`"
        @click="drillDown(group.name)"
        @keydown.enter.prevent="drillDown(group.name)"
        @keydown.space.prevent="drillDown(group.name)"
      >
        <span class="smart-views__group-name truncate-text">{{ group.name }}</span>
        <span class="smart-views__count">{{ group.taskKeys.length }}</span>
      </div>
    </div>

    <div v-if="activeKind && total > 0" class="smart-views__total">
      {{ total }} task(s) in view
    </div>

    <div v-if="!backendAvailable" class="smart-views__note">
      <i class="fas fa-circle-info"></i> evaluated locally (backend view command unavailable)
    </div>
  </div>
</template>

<style scoped>
.smart-views__title {
  display: flex;
  align-items: center;
  justify-content: space-between;
}
.smart-views__clear {
  background: none;
  border: none;
  cursor: pointer;
  color: var(--color-text-muted);
  font-size: var(--text-xs);
  padding: 0 var(--space-1);
}
.smart-views__clear:hover { color: var(--color-accent); }
.smart-views__item {
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
.smart-views__item:hover { background: var(--color-bg-hover); }
.smart-views__item--active {
  background: var(--color-bg-selected);
  color: var(--color-text-primary);
  font-weight: 600;
}
.smart-views__icon {
  font-size: var(--text-xs);
  color: var(--color-accent);
  flex-shrink: 0;
  width: 14px;
  text-align: center;
}
.smart-views__name { flex: 1; min-width: 0; }
.smart-views__count {
  font-size: var(--text-xs);
  color: var(--color-text-muted);
  background: var(--color-bg-tertiary);
  border-radius: var(--radius-full);
  padding: 0 var(--space-2);
  flex-shrink: 0;
}
.smart-views__chevron {
  font-size: 10px;
  color: var(--color-text-muted);
  flex-shrink: 0;
}
.smart-views__groups {
  margin-left: var(--space-4);
  border-left: 1px solid var(--color-border);
  padding-left: var(--space-2);
}
.smart-views__group {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: 2px var(--space-2);
  border-radius: var(--radius-sm);
  font-size: var(--text-xs);
  color: var(--color-text-secondary);
  cursor: pointer;
  min-width: 0;
}
.smart-views__group:hover { background: var(--color-bg-hover); }
.smart-views__group--active {
  background: var(--color-bg-selected);
  color: var(--color-text-primary);
  font-weight: 600;
}
.smart-views__group-name { flex: 1; min-width: 0; }
.smart-views__total {
  margin-top: var(--space-2);
  font-size: var(--text-xs);
  color: var(--color-text-muted);
}
.smart-views__note {
  margin-top: var(--space-2);
  font-size: var(--text-xs);
  color: var(--color-text-muted);
  display: flex;
  align-items: center;
  gap: var(--space-1);
}
</style>
