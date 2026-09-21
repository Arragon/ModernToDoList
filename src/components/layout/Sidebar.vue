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
        <span v-else>No Workspace</span>
      </h2>
      <div v-if="hasWorkspace()" class="sidebar__quick">
        <button
          class="sidebar__quick-btn"
          title="Global Search (Ctrl+F)"
          :disabled="documentCount === 0"
          @click="openGlobalSearch()"
        >
          <i class="fas fa-magnifying-glass"></i>
        </button>
        <button
          class="sidebar__quick-btn"
          title="Command Palette (Ctrl+K)"
          @click="openCommandPalette()"
        >
          <i class="fas fa-terminal"></i>
        </button>
      </div>
    </div>

    <div class="sidebar__content">
      <div v-if="!hasWorkspace()" class="sidebar__empty">
        <p class="sidebar__hint">Open or create a workspace to get started</p>
        <button class="btn btn-primary" @click="executeCommand('workspace.open')">
          <i class="fas fa-folder-open"></i> Open Workspace
        </button>
        <button class="btn" @click="executeCommand('workspace.create')">
          <i class="fas fa-folder-plus"></i> New Workspace
        </button>
      </div>

      <template v-else>
        <div class="sidebar__section">
          <div class="sidebar__section-title">Documents ({{ documentCount }})</div>
          <div v-if="documentCount === 0" class="sidebar__hint">
            No documents found — run Scan &amp; Index.
          </div>
          <div v-for="doc in documents" :key="doc.id" class="sidebar__item" :title="doc.file_path">
            <i class="fas fa-file-lines"></i>
            <span class="truncate-text">{{ doc.file_path }}</span>
            <span class="chip">{{ doc.doc_type }}</span>
          </div>

          <div class="sidebar__actions">
            <button class="btn" @click="scanAndIndex()" title="Scan & Index">
              <i class="fas fa-magnifying-glass"></i> Scan
            </button>
            <button class="btn" @click="rebuildIndex()" title="Rebuild Index">
              <i class="fas fa-arrows-rotate"></i> Rebuild
            </button>
          </div>
        </div>

        <SavedViewsPanel />

        <div class="sidebar__section">
          <div class="sidebar__section-title">Edit Session</div>
          <div class="sidebar__item">
            <span :class="['status-dot', isDirty ? 'status-dot--warn' : 'status-dot--ok']"></span>
            <span>{{ isDirty ? 'Unsaved changes' : 'Saved' }}</span>
          </div>
          <div class="sidebar__actions">
            <button class="btn" title="Save (Ctrl+S)" @click="executeCommand('file.save')">
              <i class="fas fa-floppy-disk"></i> Save
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
        <i class="fas fa-folder-minus"></i> Close
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
