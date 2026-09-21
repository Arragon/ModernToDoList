<script setup lang="ts">
import {
  workspace, documents, dbStatus,
  closeWorkspace, scanAndIndex, rebuildIndex,
} from "../../stores/app-state";
import { executeCommand } from "../../app/commands";

const hasWorkspace = () => workspace.value !== null;
const workspaceName = () => workspace.value?.name ?? "";
</script>

<template>
  <aside class="sidebar">
    <div class="sidebar__header">
      <h2 class="sidebar__title">
        <i class="fas fa-folder-open"></i>
        <span v-if="hasWorkspace()">{{ workspaceName() }}</span>
        <span v-else>No Workspace</span>
      </h2>
    </div>

    <div class="sidebar__content">
      <div v-if="!hasWorkspace()" class="sidebar__empty">
        <p class="sidebar__hint">Open or create a workspace to get started</p>
        <button class="btn btn-primary" @click="executeCommand('workspace.open')">
          <i class="fas fa-folder-plus"></i> New Workspace
        </button>
      </div>

      <div v-else class="sidebar__section">
        <div class="sidebar__section-title">Documents</div>
        <div v-if="documents.length === 0" class="sidebar__hint">
          No documents found
        </div>
        <div v-for="doc in documents" :key="doc.id" class="sidebar__item">
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
    </div>

    <div class="sidebar__footer">
      <button v-if="hasWorkspace()" class="btn" @click="closeWorkspace()">
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
  background: var(--color-bg-secondary);
  border-right: 1px solid var(--color-border);
  height: 100%;
  overflow: hidden;
}
.sidebar__header {
  padding: var(--space-3) var(--space-4);
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
}
.sidebar__title {
  font-size: var(--text-sm);
  font-weight: 600;
  display: flex;
  align-items: center;
  gap: var(--space-2);
  color: var(--color-text-primary);
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
  gap: var(--space-4);
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
}
.sidebar__item:hover { background: var(--color-bg-hover); }
.sidebar__label { font-size: var(--text-xs); color: var(--color-text-muted); }
.sidebar__actions { display: flex; gap: var(--space-2); margin-top: var(--space-3); }
.sidebar__footer {
  padding: var(--space-2) var(--space-3);
  border-top: 1px solid var(--color-border);
  flex-shrink: 0;
}
.status-dot { width: 8px; height: 8px; border-radius: 50%; flex-shrink: 0; }
.status-dot--ok { background: var(--color-success); }
.status-dot--error { background: var(--color-error); }
</style>
