<script setup lang="ts">
/**
 * Inspector description section (mounts the M7 rich text editor).
 *
 * Read path:  `get_task_comments` returns the verbatim COMMENTSTYPE + content.
 * Write path: `update_task_field(field = "comments")` — content only. The
 * backend has no COMMENTSTYPE write path yet, so conversion is disabled here
 * (`:convert-supported="false"`) rather than offered and silently dropped.
 *
 * Invariants honoured:
 *   - Switching tasks flushes any pending debounced commit before loading the
 *     next task, so an edit is never attributed to the wrong task.
 *   - The editor emits nothing on mount / external change, so merely selecting
 *     a task never creates a Comments element.
 */
import { onBeforeUnmount, ref, watch } from "vue";
import * as ipc from "../../ipc/client";
import { ipcFailureMessage } from "../../ipc/safe";
import { ensureSessionForDocument, noteMutation } from "../../stores/session-store";
import { showToast } from "../../stores/app-state";
import RichTextEditor from "../editor/RichTextEditor.vue";

const props = defineProps<{
  taskKey: string;
  documentId: string;
  disabled?: boolean;
}>();

const editorRef = ref<InstanceType<typeof RichTextEditor> | null>(null);

const available = ref(false);
const loading = ref(false);
const content = ref("");
const commentsType = ref<string | null>(null);

/** Token so a slow fetch for a previously selected task cannot clobber the current one. */
let loadToken = 0;

async function load(taskKey: string, documentId: string): Promise<void> {
  const token = ++loadToken;
  loading.value = true;
  available.value = false;
  content.value = "";
  commentsType.value = null;

  const sessionId = await ensureSessionForDocument(documentId);
  if (sessionId === null || token !== loadToken) {
    if (token === loadToken) loading.value = false;
    return;
  }

  const result = await ipc.getTaskComments(sessionId, taskKey);
  if (token !== loadToken) return;
  loading.value = false;

  if (!result.ok) {
    // Missing command or backend error: hide the section rather than show a
    // broken editor. The capability store already recorded a missing command.
    available.value = false;
    return;
  }

  content.value = result.value.content;
  commentsType.value = result.value.comments_type || null;
  available.value = true;
}

async function onEditorUpdate(value: string): Promise<void> {
  if (value === content.value) return;
  const sessionId = await ensureSessionForDocument(props.documentId);
  if (sessionId === null) return;

  const result = await ipc.updateTaskField(sessionId, props.taskKey, "comments", value);
  if (!result.ok) {
    showToast(ipcFailureMessage(result, "Update description"), result.missing ? "warning" : "error");
    return;
  }
  if (!result.value.success) {
    showToast("Description update was rejected by the backend", "error");
    return;
  }
  // Matches the editor's `lastEmitted`, so its watch will not re-feed content.
  content.value = value;
  await noteMutation(sessionId);
}

function onRequestConvert(): void {
  showToast("Converting a plain-text description to rich text is not yet supported", "warning");
}

watch(
  () => [props.taskKey, props.documentId] as const,
  ([taskKey, documentId]) => {
    // Flush a pending commit for the outgoing task before loading the next one.
    editorRef.value?.flush();
    void load(taskKey, documentId);
  },
  { immediate: true }
);

onBeforeUnmount(() => {
  loadToken++;
  editorRef.value?.flush();
});
</script>

<template>
  <section class="desc">
    <h3 class="desc__heading">Description</h3>
    <p v-if="loading" class="desc__hint">Loading…</p>
    <p v-else-if="!available" class="desc__hint">
      Descriptions are unavailable for this task.
    </p>
    <RichTextEditor
      v-else
      ref="editorRef"
      :model-value="content"
      :comments-type-attr="commentsType"
      :document-id="documentId"
      :disabled="disabled"
      :convert-supported="false"
      @update:model-value="onEditorUpdate"
      @request-convert="onRequestConvert"
    />
  </section>
</template>

<style scoped>
.desc {
  display: flex;
  flex-direction: column;
  gap: var(--space-2, 8px);
}
.desc__heading {
  margin: 0;
  font-size: var(--text-xs, 11px);
  font-weight: 600;
  color: var(--color-text-muted, #718096);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}
.desc__hint {
  margin: 0;
  font-size: var(--text-sm, 13px);
  color: var(--color-text-muted, #718096);
}
</style>
