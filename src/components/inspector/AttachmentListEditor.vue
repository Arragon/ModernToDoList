<script setup lang="ts">
/**
 * Attachment list editor (RD-M6-041~045).
 *
 * Type icons, display names and status indicators, plus the full action set:
 * Add Managed / Link Local / Add URL / Open / Reveal in Explorer / Rename /
 * Remove. Files dropped on the list are imported as managed attachments
 * (transactional copy on the backend); missing files render in a degraded
 * "missing" state instead of failing silently.
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import type { AttachmentDto } from "../../ipc/types";
import {
  addUrlAttachment, attachmentsAvailable, attachmentsFor, importManagedAttachment,
  linkLocalAttachment, loadAttachments, openAttachment, removeAttachment,
  renameAttachment, revealAttachment,
} from "../../stores/relation-store";
import type { TaskIdentity } from "../../stores/relation-store";
import {
  fileNameOf, formatBytes, openExternal, pathFromFileUrl, pickPath, revealInExplorer,
} from "../../app/platform";
import { showConfirm, showToast } from "../../stores/app-state";
import { Commands } from "../../ipc/commands";
import { isCommandAvailable } from "../../stores/capability-store";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import type { UnlistenFn } from "@tauri-apps/api/event";

const props = defineProps<{
  taskKey: string;
  documentId: string;
}>();

const identity = computed<TaskIdentity>(() => ({
  task_key: props.taskKey,
  document_id: props.documentId,
}));

const rows = computed<AttachmentDto[]>(() => attachmentsFor(props.taskKey));
const available = computed(() => attachmentsAvailable.value);
const busy = ref(false);
const urlFormOpen = ref(false);
const urlDraft = ref("");
const dragActive = ref(false);
const renamingId = ref<string | null>(null);
const renameDraft = ref("");

let unlistenDragDrop: UnlistenFn | null = null;
let disposed = false;

async function refresh(): Promise<void> {
  await loadAttachments(identity.value, true);
}

watch(() => [props.taskKey, props.documentId], () => {
  urlFormOpen.value = false;
  urlDraft.value = "";
  renamingId.value = null;
  void refresh();
}, { immediate: true });

// ── Drag & drop import ───────────────────────────────────────────────────────

/**
 * Tauri surfaces real filesystem paths through the webview drag-drop event;
 * the HTML5 DataTransfer API cannot, so both routes are wired.
 */
onMounted(() => {
  try {
    const webview = getCurrentWebview();
    webview
      .onDragDropEvent((event) => {
        if (event.payload.type === "enter" || event.payload.type === "over") {
          dragActive.value = true;
          return;
        }
        if (event.payload.type === "leave") {
          dragActive.value = false;
          return;
        }
        if (event.payload.type === "drop") {
          dragActive.value = false;
          void importPaths(event.payload.paths, true);
        }
      })
      .then((unlisten) => {
        if (disposed) unlisten();
        else unlistenDragDrop = unlisten;
      })
      .catch(() => {
        // Running outside Tauri (e.g. `npm run dev` in a browser): HTML5 only.
      });
  } catch {
    // `getCurrentWebview` is unavailable outside a Tauri webview.
  }
});

onBeforeUnmount(() => {
  disposed = true;
  if (unlistenDragDrop) {
    unlistenDragDrop();
    unlistenDragDrop = null;
  }
  dragActive.value = false;
});

function onHtmlDragOver(e: DragEvent): void {
  if (!available.value) return;
  e.preventDefault();
  dragActive.value = true;
}

function onHtmlDragLeave(): void {
  dragActive.value = false;
}

function onHtmlDrop(e: DragEvent): void {
  e.preventDefault();
  dragActive.value = false;
  if (!available.value) return;
  const transfer = e.dataTransfer;
  if (!transfer) return;

  const paths: string[] = [];
  const files = transfer.files;
  for (let i = 0; i < files.length; i += 1) {
    // WebView2 does not expose a real path; Electron-style builds do.
    const maybePath = (files[i] as File & { path?: string }).path;
    if (maybePath) paths.push(maybePath);
  }
  if (paths.length === 0) {
    const uriList = transfer.getData("text/uri-list");
    for (const uri of uriList.split(/\r?\n/)) {
      const local = pathFromFileUrl(uri.trim());
      if (local) paths.push(local);
    }
  }
  if (paths.length === 0) {
    showToast("Dropped items did not contain a filesystem path", "warning");
    return;
  }
  void importPaths(paths, true);
}

/** Imports dropped/picked paths as managed attachments (copies into assets). */
async function importPaths(paths: string[], managed: boolean): Promise<void> {
  if (paths.length === 0) return;
  if (!isCommandAvailable(managed ? Commands.ADD_MANAGED_ATTACHMENT : Commands.LINK_LOCAL_ATTACHMENT)) {
    showToast("Attachment import is not available in this backend build yet", "warning");
    return;
  }
  busy.value = true;
  let ok = 0;
  try {
    for (const path of paths) {
      const done = managed
        ? await importManagedAttachment(identity.value, path, fileNameOf(path))
        : await linkLocalAttachment(identity.value, path, fileNameOf(path));
      if (done) ok += 1;
    }
  } finally {
    busy.value = false;
  }
  if (ok > 0) {
    showToast(`Imported ${ok} attachment(s)`, "success");
    await refresh();
  }
}

// ── Actions ──────────────────────────────────────────────────────────────────

async function addManaged(): Promise<void> {
  const path = await pickPath({
    title: "Add managed attachment",
    mode: "file",
    message: "The file is copied into the document's .assets directory.",
  });
  if (path) await importPaths([path], true);
}

async function addLinked(): Promise<void> {
  const path = await pickPath({
    title: "Link a local file",
    mode: "file",
    message: "Only a path reference is stored — the file is not copied.",
  });
  if (path) await importPaths([path], false);
}

async function submitUrl(): Promise<void> {
  const url = urlDraft.value.trim();
  if (!url) return;
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    showToast("Enter an absolute URL (http/https)", "warning");
    return;
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
    showToast(`Unsupported scheme "${parsed.protocol}" — only http(s) is allowed`, "warning");
    return;
  }
  busy.value = true;
  try {
    const ok = await addUrlAttachment(identity.value, url, parsed.hostname);
    if (ok) {
      urlDraft.value = "";
      urlFormOpen.value = false;
      showToast("URL attachment added", "success");
      await refresh();
    }
  } finally {
    busy.value = false;
  }
}

async function open(attachment: AttachmentDto): Promise<void> {
  if (attachment.kind === "url") {
    await openExternal(attachment.path_or_url);
    return;
  }
  if (!attachment.exists) {
    showToast("The file is missing from disk", "error");
    return;
  }
  const handled = await openAttachment(identity.value, attachment);
  if (!handled) await openExternal(attachment.path_or_url);
}

async function reveal(attachment: AttachmentDto): Promise<void> {
  if (attachment.kind === "url") {
    showToast("URL attachments have no location on disk", "info");
    return;
  }
  const handled = await revealAttachment(identity.value, attachment);
  if (!handled) await revealInExplorer(attachment.path_or_url);
}

function startRename(attachment: AttachmentDto): void {
  renamingId.value = attachment.id;
  renameDraft.value = attachment.display_name;
}

async function submitRename(attachment: AttachmentDto): Promise<void> {
  const name = renameDraft.value.trim();
  if (!name) {
    showToast("A display name is required", "warning");
    return;
  }
  busy.value = true;
  try {
    const ok = await renameAttachment(identity.value, attachment, name);
    if (ok) {
      renamingId.value = null;
      showToast("Attachment renamed", "success");
    }
  } finally {
    busy.value = false;
  }
}

function confirmRemove(attachment: AttachmentDto): void {
  const semantics = attachment.kind === "managed"
    ? "The stored file becomes an orphan candidate and can be reclaimed later."
    : "Only the reference is removed — the source file is never touched.";
  showConfirm(
    "Remove attachment?",
    `Remove “${attachment.display_name}”? ${semantics}`,
    () => {
      void removeAttachment(identity.value, attachment.id).then((ok) => {
        if (ok) showToast("Attachment removed", "success");
      });
    },
    "Remove",
  );
}

// ── Presentation helpers ─────────────────────────────────────────────────────

function kindIcon(kind: AttachmentDto["kind"]): string {
  switch (kind) {
    case "managed": return "fas fa-box-archive";
    case "linked": return "fas fa-link";
    case "url": return "fas fa-globe";
    default: return "fas fa-paperclip";
  }
}

function kindLabel(kind: AttachmentDto["kind"]): string {
  switch (kind) {
    case "managed": return "Managed";
    case "linked": return "Linked";
    case "url": return "URL";
    default: return kind;
  }
}

function statusOf(attachment: AttachmentDto): { label: string; icon: string; tone: string } {
  if (attachment.kind === "url") {
    return { label: "Link", icon: "fas fa-globe", tone: "" };
  }
  if (!attachment.exists || attachment.status === "missing") {
    return { label: "Missing file", icon: "fas fa-triangle-exclamation", tone: "error" };
  }
  if (attachment.status === "unverified") {
    return { label: "Unverified", icon: "fas fa-circle-question", tone: "warning" };
  }
  if (attachment.status === "orphaned") {
    return { label: "Orphan candidate", icon: "fas fa-ghost", tone: "warning" };
  }
  return { label: attachment.hash ? "Verified" : "OK", icon: "fas fa-circle-check", tone: "success" };
}

function rowClass(attachment: AttachmentDto): string {
  const tone = statusOf(attachment).tone;
  if (tone === "error") return "rel-editor__row rel-editor__row--error";
  if (tone === "warning") return "rel-editor__row rel-editor__row--warning";
  return "rel-editor__row";
}

function shortHash(hash: string | null): string {
  return hash ? hash.slice(0, 8) : "";
}
</script>

<template>
  <div
    class="rel-editor attachment-editor"
    :class="{ 'attachment-editor--drag': dragActive }"
    @dragover="onHtmlDragOver"
    @dragleave="onHtmlDragLeave"
    @drop="onHtmlDrop"
  >
    <div class="rel-editor__header">
      <span class="rel-editor__title">
        <i class="fas fa-paperclip"></i> Attachments
      </span>
      <span class="rel-editor__count">{{ rows.length }}</span>
      <span class="rel-editor__spacer"></span>
      <button
        class="rel-editor__icon-btn"
        :disabled="!available"
        title="Reload attachments"
        @click="refresh"
      >
        <i class="fas fa-rotate"></i>
      </button>
    </div>

    <div class="attachment-editor__actions">
      <button class="btn" :disabled="!available || busy" title="Copy into .assets" @click="addManaged">
        <i class="fas fa-box-archive"></i> Add Managed
      </button>
      <button class="btn" :disabled="!available || busy" title="Store a path reference" @click="addLinked">
        <i class="fas fa-link"></i> Link Local
      </button>
      <button class="btn" :disabled="!available || busy" @click="urlFormOpen = !urlFormOpen">
        <i class="fas fa-globe"></i> Add URL
      </button>
    </div>

    <div v-if="urlFormOpen" class="attachment-editor__url">
      <input
        v-model="urlDraft"
        class="rel-editor__input"
        type="text"
        placeholder="https://example.com/report.pdf"
        spellcheck="false"
        :disabled="busy"
        @keydown.enter.prevent="submitUrl"
      />
      <button class="btn btn-primary" :disabled="busy || !urlDraft.trim()" @click="submitUrl">
        <i class="fas fa-plus"></i>
      </button>
    </div>

    <div v-if="rows.length === 0" class="rel-editor__empty">
      No attachments. Drop a file here to import it.
    </div>

    <div v-else class="rel-editor__list">
      <div v-for="attachment in rows" :key="attachment.id" :class="rowClass(attachment)">
        <i :class="['attachment-editor__kind', kindIcon(attachment.kind)]" :title="kindLabel(attachment.kind)"></i>

        <template v-if="renamingId === attachment.id">
          <input
            v-model="renameDraft"
            class="rel-editor__input"
            type="text"
            :disabled="busy"
            @keydown.enter.prevent="submitRename(attachment)"
            @keydown.esc.prevent="renamingId = null"
          />
          <button class="rel-editor__icon-btn" title="Save" :disabled="busy" @click="submitRename(attachment)">
            <i class="fas fa-check"></i>
          </button>
          <button class="rel-editor__icon-btn" title="Cancel" :disabled="busy" @click="renamingId = null">
            <i class="fas fa-xmark"></i>
          </button>
        </template>

        <template v-else>
          <span class="rel-editor__row-label" :title="attachment.path_or_url">
            {{ attachment.display_name }}
          </span>
          <span class="attachment-editor__status" :class="`attachment-editor__status--${statusOf(attachment).tone || 'plain'}`">
            <i :class="statusOf(attachment).icon"></i>
            {{ statusOf(attachment).label }}
          </span>
          <span v-if="attachment.size !== null" class="rel-editor__row-sub">
            {{ formatBytes(attachment.size) }}
          </span>
          <span v-if="shortHash(attachment.hash)" class="rel-editor__row-sub attachment-editor__hash" :title="`BLAKE3 ${attachment.hash}`">
            {{ shortHash(attachment.hash) }}
          </span>

          <button class="rel-editor__icon-btn" title="Open" :disabled="busy" @click="open(attachment)">
            <i class="fas fa-arrow-up-right-from-square"></i>
          </button>
          <button
            v-if="attachment.kind !== 'url'"
            class="rel-editor__icon-btn"
            title="Reveal in Explorer"
            :disabled="busy || !attachment.exists"
            @click="reveal(attachment)"
          >
            <i class="fas fa-folder-open"></i>
          </button>
          <button
            class="rel-editor__icon-btn"
            title="Rename"
            :disabled="!available || busy"
            @click="startRename(attachment)"
          >
            <i class="fas fa-pen"></i>
          </button>
          <button
            class="rel-editor__icon-btn rel-editor__icon-btn--danger"
            title="Remove"
            :disabled="!available || busy"
            @click="confirmRemove(attachment)"
          >
            <i class="fas fa-trash"></i>
          </button>
        </template>
      </div>
    </div>

    <div v-if="dragActive" class="attachment-editor__drop-hint">
      <i class="fas fa-file-import"></i> Drop to import as managed attachments
    </div>

    <div v-if="!available" class="rel-editor__note rel-editor__note--warning">
      <i class="fas fa-triangle-exclamation"></i>
      Attachment commands are not available in this backend build yet.
    </div>
  </div>
</template>

<style scoped>
.attachment-editor {
  position: relative;
}
.attachment-editor--drag {
  outline: 2px dashed var(--color-accent);
  outline-offset: -2px;
  border-radius: var(--radius-md);
}
.attachment-editor__actions {
  display: flex;
  gap: var(--space-1);
  flex-wrap: wrap;
}
.attachment-editor__actions .btn {
  padding: var(--space-1) var(--space-2);
  font-size: var(--text-xs);
}
.attachment-editor__actions .btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.attachment-editor__url {
  display: flex;
  gap: var(--space-2);
}
.attachment-editor__kind {
  width: 16px;
  text-align: center;
  color: var(--color-text-secondary);
  flex-shrink: 0;
}
.attachment-editor__status {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  font-size: var(--text-xs);
  flex-shrink: 0;
}
.attachment-editor__status--success { color: var(--color-success); }
.attachment-editor__status--warning { color: var(--color-warning); }
.attachment-editor__status--error { color: var(--color-error); }
.attachment-editor__status--plain { color: var(--color-text-muted); }
.attachment-editor__hash {
  font-family: var(--font-mono);
}
.attachment-editor__drop-hint {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-2);
  background: var(--color-accent-light);
  color: var(--color-accent);
  font-size: var(--text-sm);
  font-weight: 600;
  border-radius: var(--radius-md);
  pointer-events: none;
}
</style>
