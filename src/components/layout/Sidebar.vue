<script setup lang="ts">
import { computed } from "vue";
import SavedViewsPanel from "./SavedViewsPanel.vue";
import {
  workspace, documents, dbStatus,
  closeWorkspace, scanAndIndex, rebuildIndex,
} from "../../stores/app-state";
import { canRedo, canUndo, closeAllSessions, isDirty } from "../../stores/session-store";
import { openCommandPalette, openGlobalSearch } from "../../stores/ui-store";
import { executeCommand } from "../../app/commands";
import { backendAvailable } from "../../app/backend";
import { useT } from "../../app/i18n";
import { locale, theme, toggleLocale, toggleTheme } from "../../stores/ui-prefs";

const t = useT();

const hasWorkspace = () => workspace.value !== null;
const workspaceName = () => workspace.value?.name ?? "";
const documentCount = computed(() => documents.value.length);

async function closeCurrentWorkspace() {
  await closeAllSessions();
  await closeWorkspace();
}
</script>

<template>
  <aside class="sidebar">
    <div class="sidebar__header">
      <h2 class="sidebar__title">
        <i class="fas fa-folder-open"></i>
        <span v-if="hasWorkspace()">{{ workspaceName() }}</span>
        <span v-else>{{ t('sidebar.noWorkspace') }}</span>
      </h2>
      <div v-if="hasWorkspace()" class="sidebar__quick">
        <button
          class="sidebar__quick-btn"
          :title="t('sidebar.tooltip.search')"
          :disabled="documentCount === 0"
          @click="openGlobalSearch()"
        >
          <i class="fas fa-magnifying-glass"></i>
        </button>
        <button
          class="sidebar__quick-btn"
          :title="t('sidebar.tooltip.palette')"
          @click="openCommandPalette()"
        >
          <i class="fas fa-terminal"></i>
        </button>
      </div>
      <div class="sidebar__quick sidebar__prefs">
        <button
          class="sidebar__quick-btn"
          :title="t('prefs.language')"
          @click="toggleLocale()"
        >
          <i class="fas fa-language"></i>
          <span class="sidebar__prefs-label">{{ locale === 'zh' ? 'EN' : '中' }}</span>
        </button>
        <button
          class="sidebar__quick-btn"
          :title="t('prefs.theme')"
          @click="toggleTheme()"
        >
          <i :class="theme === 'dark' ? 'fas fa-sun' : 'fas fa-moon'"></i>
        </button>
      </div>
    </div>

    <div class="sidebar__content">
      <div v-if="!hasWorkspace()" class="sidebar__empty">
        <p v-if="!backendAvailable" class="sidebar__web-notice">{{ t('web.notice') }}</p>
        <p class="sidebar__hint">{{ t('sidebar.emptyHint') }}</p>
        <button class="btn btn-primary" @click="executeCommand('workspace.open')">
          <i class="fas fa-folder-open"></i> {{ t('sidebar.openWorkspace') }}
        </button>
        <button class="btn" @click="executeCommand('workspace.create')">
          <i class="fas fa-folder-plus"></i> {{ t('sidebar.newWorkspace') }}
        </button>
      </div>

      <template v-else>
        <div class="sidebar__section">
          <div class="sidebar__section-title">{{ t('sidebar.documents') }} ({{ documentCount }})</div>
          <div v-if="documentCount === 0" class="sidebar__hint">
            {{ t('sidebar.noDocuments') }}
          </div>
          <div v-for="doc in documents" :key="doc.id" class="sidebar__item" :title="doc.file_path">
            <i class="fas fa-file-lines"></i>
            <span class="truncate-text">{{ doc.file_path }}</span>
            <span class="chip">{{ doc.doc_type }}</span>
          </div>

          <div class="sidebar__actions">
            <button class="btn" @click="scanAndIndex()" :title="t('action.scanIndex')">
              <i class="fas fa-magnifying-glass"></i> {{ t('action.scanIndex') }}
            </button>
            <button class="btn" @click="rebuildIndex()" :title="t('sidebar.tooltip.rebuild')">
              <i class="fas fa-arrows-rotate"></i> {{ t('sidebar.rebuild') }}
            </button>
          </div>
        </div>

        <SavedViewsPanel />

        <div class="sidebar__section">
          <div class="sidebar__section-title">{{ t('sidebar.editSession') }}</div>
          <div class="sidebar__item">
            <span :class="['status-dot', isDirty ? 'status-dot--warn' : 'status-dot--ok']"></span>
            <span>{{ isDirty ? t('sidebar.unsaved') : t('sidebar.saved') }}</span>
          </div>
          <div class="sidebar__actions">
            <button class="btn" :title="t('sidebar.tooltip.save')" @click="executeCommand('file.save')">
              <i class="fas fa-floppy-disk"></i> {{ t('action.save') }}
            </button>
            <button
              class="btn"
              title="Undo (Ctrl+Z)"
              :disabled="!canUndo"
              @click="executeCommand('edit.undo')"
            >
              <i class="fas fa-rotate-left"></i>
            </button>
            <button
              class="btn"
              title="Redo (Ctrl+Y)"
              :disabled="!canRedo"
              @click="executeCommand('edit.redo')"
            >
              <i class="fas fa-rotate-right"></i>
            </button>
          </div>
        </div>

        <div v-if="dbStatus" class="sidebar__section">
          <div class="sidebar__section-title">Database</div>
          <div class="sidebar__item">
            <span :class="['status-dot', dbStatus.available ? 'status-dot--ok' : 'status-dot--error']"></span>
            <span>{{ dbStatus.available ? 'Connected' : 'Recovery Mode' }}</span>
          </div>
          <div class="sidebar__item">
            <span class="sidebar__label">Schema v{{ dbStatus.schema_version }}</span>
          </div>
        </div>
      </template>
    </div>

    <div class="sidebar__footer">
      <button v-if="hasWorkspace()" class="btn" @click="closeCurrentWorkspace()">
        <i class="fas fa-folder-minus"></i> {{ t('sidebar.closeWorkspace') }}
      </button>
    </div>
  </aside>
</template>

<style scoped>
.sidebar {
  display: flex;
  flex-direction: column;
  width: var(--sidebar-width);
  min-width: var(--sidebar-min-width);
  max-width: var(--sidebar-max-width);
  flex-shrink: 0;
  background: var(--color-bg-secondary);
  border-right: 1px solid var(--color-border);
  height: 100%;
  overflow: hidden;
}
.sidebar__header {
  padding: var(--space-3) var(--space-4);
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: var(--space-2);
}
.sidebar__title {
  font-size: var(--text-sm);
  font-weight: 600;
  display: flex;
  align-items: center;
  gap: var(--space-2);
  color: var(--color-text-primary);
  flex: 1;
  min-width: 0;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}
.sidebar__quick {
  display: flex;
  gap: 2px;
  flex-shrink: 0;
}
.sidebar__quick-btn {
  background: none;
  border: none;
  cursor: pointer;
  color: var(--color-text-muted);
  padding: var(--space-1);
  border-radius: var(--radius-sm);
  font-size: var(--text-sm);
}
.sidebar__quick-btn:hover:not(:disabled) {
  color: var(--color-accent);
  background: var(--color-bg-hover);
}
.sidebar__quick-btn:disabled {
  color: var(--color-text-disabled);
  cursor: not-allowed;
}
.sidebar__prefs-label {
  font-size: var(--text-xs);
  font-weight: 600;
  margin-left: 2px;
}
.sidebar__content {
  flex: 1;
  overflow-y: auto;
  padding: var(--space-3);
}
.sidebar__empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-8) var(--space-4);
  text-align: center;
}
.sidebar__hint {
  font-size: var(--text-xs);
  color: var(--color-text-muted);
}
.sidebar__web-notice {
  font-size: var(--text-xs);
  color: var(--color-warning, #b45309);
  background: var(--color-bg-secondary);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  padding: var(--space-2);
  margin-bottom: var(--space-3);
}
.sidebar__section { margin-bottom: var(--space-4); }
.sidebar__section-title {
  font-size: var(--text-xs);
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--color-text-muted);
  margin-bottom: var(--space-2);
}
.sidebar__item {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-1) var(--space-2);
  border-radius: var(--radius-sm);
  font-size: var(--text-sm);
  color: var(--color-text-secondary);
  min-width: 0;
}
.sidebar__item:hover { background: var(--color-bg-hover); }
.sidebar__label { font-size: var(--text-xs); color: var(--color-text-muted); }
.sidebar__actions {
  display: flex;
  gap: var(--space-2);
  margin-top: var(--space-3);
  flex-wrap: wrap;
}
.sidebar__actions .btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.sidebar__footer {
  padding: var(--space-2) var(--space-3);
  border-top: 1px solid var(--color-border);
  flex-shrink: 0;
}
.status-dot { width: 8px; height: 8px; border-radius: 50%; flex-shrink: 0; }
.status-dot--ok { background: var(--color-success); }
.status-dot--warn { background: var(--color-warning); }
.status-dot--error { background: var(--color-error); }
</style>
