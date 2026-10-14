<script setup lang="ts">
/**
 * Progress links editor (RD-M6-027~030).
 *
 * Provider icon + label + URL, with add / edit / remove and an "open" action
 * that hands the URL to the Windows default browser through tauri-plugin-shell.
 * Only http(s) URLs are accepted client-side; the backend re-validates.
 */
import { computed, ref, watch } from "vue";
import type { ProgressLinkDto } from "../../ipc/types";
import {
  addProgressLink, loadProgressLinks, progressLinksAvailable, progressLinksFor,
  removeProgressLink, updateProgressLink,
} from "../../stores/relation-store";
import type { TaskIdentity } from "../../stores/relation-store";
import { detectProvider, providerIcon, providerLabel, validateProgressUrl } from "../../app/progress-links";
import { openExternal } from "../../app/platform";
import { showConfirm, showToast } from "../../stores/app-state";

const props = defineProps<{
  taskKey: string;
  documentId: string;
}>();

const identity = computed<TaskIdentity>(() => ({
  task_key: props.taskKey,
  document_id: props.documentId,
}));

const rows = computed<ProgressLinkDto[]>(() => progressLinksFor(props.taskKey));
const available = computed(() => progressLinksAvailable.value);

const busy = ref(false);
const addOpen = ref(false);
const draftLabel = ref("");
const draftUrl = ref("");
const draftError = ref<string | null>(null);
const editingId = ref<string | null>(null);
const editLabel = ref("");
const editUrl = ref("");

const draftProvider = computed(() => detectProvider(draftUrl.value));

async function refresh(): Promise<void> {
  await loadProgressLinks(identity.value, true);
}

watch(() => [props.taskKey, props.documentId], () => {
  resetDraft();
  editingId.value = null;
  void refresh();
}, { immediate: true });

function resetDraft(): void {
  draftLabel.value = "";
  draftUrl.value = "";
  draftError.value = null;
  addOpen.value = false;
}

function normaliseUrl(raw: string): string {
  const value = raw.trim();
  if (!value) return value;
  if (/^[a-z][a-z0-9+.-]*:/i.test(value)) return value;
  return `https://${value}`;
}

async function submitAdd(): Promise<void> {
  const url = normaliseUrl(draftUrl.value);
  const check = validateProgressUrl(url);
  if (!check.valid) {
    draftError.value = check.reason;
    return;
  }
  const label = draftLabel.value.trim() || hostFallbackLabel(url);
  busy.value = true;
  try {
    const ok = await addProgressLink(identity.value, label, url, detectProvider(url));
    if (ok) {
      resetDraft();
      showToast("Progress link added", "success");
    }
  } finally {
    busy.value = false;
  }
}

function hostFallbackLabel(url: string): string {
  try {
    return new URL(url).hostname;
  } catch {
    return url;
  }
}

function startEdit(link: ProgressLinkDto): void {
  editingId.value = link.id;
  editLabel.value = link.label;
  editUrl.value = link.url;
  draftError.value = null;
}

function cancelEdit(): void {
  editingId.value = null;
  draftError.value = null;
}

async function submitEdit(link: ProgressLinkDto): Promise<void> {
  const url = normaliseUrl(editUrl.value);
  const check = validateProgressUrl(url);
  if (!check.valid) {
    draftError.value = check.reason;
    return;
  }
  busy.value = true;
  try {
    const ok = await updateProgressLink(identity.value, link, editLabel.value.trim() || url, url);
    if (ok) {
      editingId.value = null;
      showToast("Progress link updated", "success");
    }
  } finally {
    busy.value = false;
  }
}

function confirmRemove(link: ProgressLinkDto): void {
  showConfirm(
    "Remove progress link?",
    `“${link.label}” will no longer be attached to this task.`,
    () => {
      void removeProgressLink(identity.value, link.id).then((ok) => {
        if (ok) showToast("Progress link removed", "success");
      });
    },
    "Remove",
  );
}

async function open(link: ProgressLinkDto): Promise<void> {
  await openExternal(link.url);
}
</script>

<template>
  <div class="rel-editor">
    <div class="rel-editor__header">
      <span class="rel-editor__title">
        <i class="fas fa-arrow-up-right-from-square"></i> Progress Links
      </span>
      <span class="rel-editor__count">{{ rows.length }}</span>
      <span class="rel-editor__spacer"></span>
      <button
        class="rel-editor__icon-btn"
        :disabled="!available"
        title="Reload links"
        @click="refresh"
      >
        <i class="fas fa-rotate"></i>
      </button>
      <button
        class="btn"
        :disabled="!available || busy"
        @click="addOpen = !addOpen"
      >
        <i :class="addOpen ? 'fas fa-xmark' : 'fas fa-plus'"></i>
        {{ addOpen ? 'Cancel' : 'Add' }}
      </button>
    </div>

    <div v-if="rows.length === 0" class="rel-editor__empty">
      No progress links yet — link a PR, issue or build.
    </div>

    <div v-else class="rel-editor__list">
      <div v-for="link in rows" :key="link.id" class="rel-editor__row">
        <template v-if="editingId === link.id">
          <i :class="['link-editor__provider', providerIcon(link.provider, link.url)]"></i>
          <input v-model="editLabel" class="rel-editor__input" type="text" placeholder="Label" />
          <input v-model="editUrl" class="rel-editor__input" type="text" placeholder="https://…" />
          <button class="rel-editor__icon-btn" title="Save" :disabled="busy" @click="submitEdit(link)">
            <i class="fas fa-check"></i>
          </button>
          <button class="rel-editor__icon-btn" title="Cancel" :disabled="busy" @click="cancelEdit">
            <i class="fas fa-xmark"></i>
          </button>
        </template>
        <template v-else>
          <i
            :class="['link-editor__provider', providerIcon(link.provider, link.url)]"
            :title="providerLabel(link.provider, link.url)"
          ></i>
          <span class="rel-editor__row-label" :title="link.url">{{ link.label }}</span>
          <span class="chip link-editor__provider-chip">
            {{ providerLabel(link.provider, link.url) }}
          </span>
          <button class="rel-editor__icon-btn" title="Open in default browser" @click="open(link)">
            <i class="fas fa-arrow-up-right-from-square"></i>
          </button>
          <button
            class="rel-editor__icon-btn"
            title="Edit"
            :disabled="!available || busy"
            @click="startEdit(link)"
          >
            <i class="fas fa-pen"></i>
          </button>
          <button
            class="rel-editor__icon-btn rel-editor__icon-btn--danger"
            title="Remove"
            :disabled="!available || busy"
            @click="confirmRemove(link)"
          >
            <i class="fas fa-trash"></i>
          </button>
        </template>
      </div>
    </div>

    <div v-if="addOpen" class="link-editor__form">
      <input
        v-model="draftLabel"
        class="rel-editor__input"
        type="text"
        placeholder="Label (e.g. PR #482)"
        :disabled="busy"
      />
      <input
        v-model="draftUrl"
        class="rel-editor__input"
        type="text"
        placeholder="https://github.com/org/repo/pull/482"
        spellcheck="false"
        :disabled="busy"
        @keydown.enter.prevent="submitAdd"
      />
      <div class="rel-editor__actions">
        <span class="rel-editor__note">
          <i :class="['link-editor__provider', providerIcon(draftProvider, draftUrl)]"></i>
          Detected: {{ providerLabel(draftProvider, draftUrl) }}
        </span>
        <span class="rel-editor__spacer"></span>
        <button
          class="btn btn-primary"
          :disabled="busy || !draftUrl.trim()"
          @click="submitAdd"
        >
          <i :class="busy ? 'fas fa-spinner fa-spin' : 'fas fa-plus'"></i> Add link
        </button>
      </div>
    </div>

    <div v-if="draftError" class="rel-editor__note rel-editor__note--error">
      <i class="fas fa-circle-exclamation"></i> {{ draftError }}
    </div>

    <div v-if="!available" class="rel-editor__note rel-editor__note--warning">
      <i class="fas fa-triangle-exclamation"></i>
      Progress link commands are not available in this backend build yet.
    </div>
  </div>
</template>

<style scoped>
.link-editor__provider {
  width: 16px;
  text-align: center;
  color: var(--color-text-secondary);
  flex-shrink: 0;
}
.link-editor__provider-chip {
  flex-shrink: 0;
}
.link-editor__form {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  border-top: 1px dashed var(--color-border);
  padding-top: var(--space-2);
}
</style>
