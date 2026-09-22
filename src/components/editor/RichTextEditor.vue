<script setup lang="ts">
/**
 * Rich text editor for task descriptions (RD-M7-001~002, RD-M7-014~018,
 * RD-M7-026~027).
 *
 * Mode is driven entirely by the task's COMMENTSTYPE:
 *   PLAIN_TEXT -> plain textarea; saving emits raw text so COMMENTSTYPE is
 *                 never changed by editing
 *   HTML       -> Tiptap editor with explicit Edit / Preview modes
 *   RTF, and any unrecognised value -> read-only excerpt with a type indicator;
 *                 no edit and no convert affordance is offered
 *
 * Two invariants this component must not break:
 *   1. Merely viewing a task never creates or converts its Comments type. The
 *      component emits nothing on mount and nothing when `modelValue` changes
 *      from outside — only a user edit emits.
 *   2. Commits to Core are coalesced. Tiptap's own history handles granular
 *      text undo locally; a single debounced emit follows the last keystroke so
 *      the caller performs one undoable field update rather than one per key
 *      (QA-M7-016 forbids serializing per keystroke).
 *
 * All four sanitizer choke points are wired: paste import, Core -> Editor feed,
 * Editor -> Core commit, and preview render.
 */
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { EditorContent, useEditor } from "@tiptap/vue-3";
import RichTextToolbar from "./RichTextToolbar.vue";
import { buildEditorExtensions } from "./extensions";
import {
  canConvertToRichText,
  editorModeFor,
  isReadOnly,
  parseCommentsType,
  plainTextToPreviewHtml,
  readOnlyExcerpt,
  typeIndicator,
} from "./comments-model";
import {
  sanitizeForCore,
  sanitizeForEditor,
  sanitizeForPasteImport,
  sanitizeForPreview,
} from "./sanitize";

const props = withDefaults(
  defineProps<{
    /** Stored comment content: raw text for PLAIN_TEXT, HTML for HTML. */
    modelValue: string;
    /** Verbatim COMMENTSTYPE attribute, or null when absent. */
    commentsTypeAttr?: string | null;
    /** Pins managed image references during sanitisation. */
    documentId?: string | null;
    disabled?: boolean;
    /**
     * Whether the owner can actually persist a COMMENTSTYPE change. The backend
     * has no write path for the type attribute yet, so the Inspector passes
     * false and the "Convert to Rich Text" affordance is hidden rather than
     * shown broken. Flip to true once conversion is wired end to end.
     */
    convertSupported?: boolean;
  }>(),
  { commentsTypeAttr: null, documentId: null, disabled: false, convertSupported: true }
);

const emit = defineEmits<{
  (e: "update:modelValue", value: string): void;
  /** Asks the owner to run the confirmation-gated, undoable conversion. */
  (e: "request-convert"): void;
}>();

const commentsType = computed(() => parseCommentsType(props.commentsTypeAttr));
const mode = computed(() => editorModeFor(commentsType.value));
const readOnly = computed(() => isReadOnly(commentsType.value) || props.disabled);
const canConvert = computed(() => canConvertToRichText(commentsType.value) && props.convertSupported && !props.disabled);
const indicator = computed(() => typeIndicator(commentsType.value));

// ── Edit / Preview ───────────────────────────────────────────────────────────
const previewing = ref(false);

/** Preview HTML, sanitised on the way in even though we produced it ourselves. */
const previewHtml = computed(() => {
  if (mode.value === "plain_text") return plainTextToPreviewHtml(props.modelValue);
  if (mode.value === "read_only") return "";
  const live = editor.value?.getHTML();
  return sanitizeForPreview(live ?? props.modelValue, props.documentId);
});

const readOnlyText = computed(() => readOnlyExcerpt(props.modelValue));

// ── Plain text mode ──────────────────────────────────────────────────────────
const plainDraft = ref(props.modelValue);
watch(
  () => props.modelValue,
  (next) => {
    // An external change (task switch, undo, reload) is not a user edit, so it
    // must not be echoed back — that would risk creating a Comments element on
    // a task that never had one.
    if (next !== plainDraft.value) plainDraft.value = next;
  }
);

function onPlainInput(event: Event): void {
  plainDraft.value = (event.target as HTMLTextAreaElement).value;
  schedulePlainCommit();
}

let plainTimer: number | undefined;
function schedulePlainCommit(): void {
  window.clearTimeout(plainTimer);
  plainTimer = window.setTimeout(() => {
    // Emitted as raw text: no HTML wrapping, so COMMENTSTYPE stays PLAIN_TEXT.
    emit("update:modelValue", plainDraft.value);
  }, COMMIT_DEBOUNCE_MS);
}

// ── Rich text mode ───────────────────────────────────────────────────────────
const COMMIT_DEBOUNCE_MS = 1500;
let commitTimer: number | undefined;
/** Last value we emitted, so an external echo is not treated as a new edit. */
let lastEmitted: string | null = null;

const linkPromptOpen = ref(false);
const linkUrl = ref("");

const editor = useEditor({
  // Core -> Editor: stored content is sanitised before it is ever editable.
  content: sanitizeForEditor(props.modelValue, props.documentId),
  extensions: buildEditorExtensions(() => {
    linkUrl.value = currentHref();
    linkPromptOpen.value = true;
  }),
  editorProps: {
    attributes: { class: "rte-content", spellcheck: "true" },
    // Paste import choke point.
    transformPastedHTML: (html: string) => sanitizeForPasteImport(html, props.documentId),
  },
  onUpdate: ({ editor: instance }) => {
    scheduleRichCommit(instance.getHTML());
  },
});

function scheduleRichCommit(html: string): void {
  window.clearTimeout(commitTimer);
  commitTimer = window.setTimeout(() => {
    // Editor -> Core choke point: only whitelisted markup is ever persisted.
    const clean = sanitizeForCore(html, props.documentId);
    lastEmitted = clean;
    emit("update:modelValue", clean);
  }, COMMIT_DEBOUNCE_MS);
}

/** Flush a pending commit immediately (task switch, window close, save). */
function flush(): void {
  if (commitTimer !== undefined) {
    window.clearTimeout(commitTimer);
    commitTimer = undefined;
    const instance = editor.value;
    if (instance) {
      const clean = sanitizeForCore(instance.getHTML(), props.documentId);
      lastEmitted = clean;
      emit("update:modelValue", clean);
    }
  }
  if (plainTimer !== undefined) {
    window.clearTimeout(plainTimer);
    plainTimer = undefined;
    emit("update:modelValue", plainDraft.value);
  }
}

defineExpose({ flush });

watch(
  () => props.modelValue,
  (next) => {
    const instance = editor.value;
    if (!instance || mode.value !== "rich_text") return;
    if (next === lastEmitted) return; // our own commit echoed back
    const current = sanitizeForEditor(next, props.documentId);
    if (current !== instance.getHTML()) {
      instance.commands.setContent(current, { emitUpdate: false });
    }
  }
);

// ── Link control ─────────────────────────────────────────────────────────────
function currentHref(): string {
  const attrs = editor.value?.getAttributes("link");
  return typeof attrs?.href === "string" ? attrs.href : "";
}

function applyLink(): void {
  const instance = editor.value;
  const url = linkUrl.value.trim();
  linkPromptOpen.value = false;
  if (!instance) return;
  if (url === "") {
    instance.chain().focus().unsetLink().run();
    return;
  }
  // Only http/https survive the backend sanitizer; refuse anything else here so
  // the user is not silently stripped on save.
  if (!/^https?:\/\//i.test(url)) return;
  instance.chain().focus().setLink({ href: url }).run();
}

function clearLink(): void {
  linkUrl.value = "";
  applyLink();
}

onBeforeUnmount(() => {
  window.clearTimeout(commitTimer);
  window.clearTimeout(plainTimer);
  editor.value?.destroy();
});
</script>

<template>
  <div class="rte" :data-mode="mode">
    <header class="rte__head">
      <span class="rte__type" :title="indicator">{{ indicator }}</span>

      <div class="rte__actions">
        <template v-if="mode === 'rich_text'">
          <button type="button" class="rte__toggle" :aria-pressed="!previewing" :disabled="readOnly"
            @click="previewing = false">Edit</button>
          <button type="button" class="rte__toggle" :aria-pressed="previewing" :disabled="readOnly"
            @click="previewing = true">Preview</button>
        </template>

        <button v-if="canConvert" type="button" class="rte__convert" :disabled="disabled"
          title="Converts PLAIN_TEXT to HTML. The owner confirms first and the change is undoable."
          @click="emit('request-convert')">Convert to Rich Text</button>
      </div>
    </header>

    <!-- RTF and unrecognised COMMENTSTYPE values: read-only, no edit, no convert. -->
    <div v-if="mode === 'read_only'" class="rte__readonly">
      <p class="rte__notice">
        This description uses a format ModernToDoList does not edit. It is preserved exactly as
        stored and will not be altered or converted.
      </p>
      <pre class="rte__excerpt">{{ readOnlyText }}</pre>
    </div>

    <!-- PLAIN_TEXT: raw text in, raw text out; COMMENTSTYPE is never changed. -->
    <textarea v-else-if="mode === 'plain_text'" class="rte__plain" :value="plainDraft" :disabled="readOnly"
      rows="8" aria-label="Description (plain text)" @input="onPlainInput" @blur="flush" />

    <!-- HTML: Tiptap with the approved toolbar, plus a separate preview path. -->
    <template v-else>
      <RichTextToolbar v-if="!previewing" :editor="editor" :disabled="readOnly"
        @request-link="linkUrl = currentHref(); linkPromptOpen = true" />

      <div v-if="linkPromptOpen" class="rte__link">
        <input v-model="linkUrl" type="url" class="rte__link-input" placeholder="https://example.com"
          aria-label="Link URL" @keydown.enter.prevent="applyLink" @keydown.esc.prevent="linkPromptOpen = false" />
        <button type="button" class="rte__toggle" @click="applyLink">Apply</button>
        <button type="button" class="rte__toggle" :disabled="linkUrl === ''" @click="clearLink">Remove</button>
        <button type="button" class="rte__toggle" @click="linkPromptOpen = false">Cancel</button>
        <span class="rte__hint">Only http and https links are kept.</span>
      </div>

      <EditorContent v-show="!previewing" class="rte__editor" :editor="editor" />
      <div v-if="previewing" class="rte__preview" @blur="flush" tabindex="0">
        <!-- Separate, sanitised rendering path: never the live editor DOM. -->
        <div class="rte-preview-body" v-html="previewHtml" />
      </div>
    </template>
  </div>
</template>

<style scoped>
.rte {
  display: flex;
  flex-direction: column;
  gap: var(--space-2, 8px);
}

.rte__head {
  display: flex;
  gap: var(--space-2, 8px);
  align-items: center;
  justify-content: space-between;
}

.rte__type {
  font-size: 11px;
  color: var(--color-text-muted, #718096);
  letter-spacing: 0.02em;
}

.rte__actions {
  display: flex;
  gap: 4px;
  align-items: center;
}

.rte__toggle,
.rte__convert {
  height: 24px;
  padding: 0 8px;
  font: inherit;
  font-size: 12px;
  color: var(--color-text-primary, #1a1a2e);
  cursor: pointer;
  background: var(--color-background, #fff);
  border: 1px solid var(--color-border, #d9d9e3);
  border-radius: var(--radius-sm, 4px);
}

.rte__toggle[aria-pressed="true"] {
  color: var(--color-accent, #5a67d8);
  background: var(--color-selected, #e6e8fb);
  border-color: var(--color-accent, #5a67d8);
}

.rte__toggle:disabled,
.rte__convert:disabled {
  cursor: default;
  opacity: 0.45;
}

.rte__convert:hover:not(:disabled) {
  border-color: var(--color-accent, #5a67d8);
}

.rte__plain,
.rte__excerpt {
  width: 100%;
  padding: var(--space-2, 8px);
  font: inherit;
  font-size: 13px;
  line-height: 1.55;
  color: var(--color-text-primary, #1a1a2e);
  resize: vertical;
  background: var(--color-background, #fff);
  border: 1px solid var(--color-border, #d9d9e3);
  border-radius: var(--radius-sm, 4px);
}

.rte__excerpt {
  min-height: 72px;
  margin: 0;
  overflow-wrap: anywhere;
  white-space: pre-wrap;
}

.rte__readonly {
  display: flex;
  flex-direction: column;
  gap: var(--space-2, 8px);
}

.rte__notice {
  margin: 0;
  font-size: 12px;
  color: var(--color-text-muted, #718096);
}

.rte__link {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  align-items: center;
}

.rte__link-input {
  flex: 1 1 180px;
  height: 24px;
  padding: 0 6px;
  font: inherit;
  font-size: 12px;
  border: 1px solid var(--color-border, #d9d9e3);
  border-radius: var(--radius-sm, 4px);
}

.rte__hint {
  font-size: 11px;
  color: var(--color-text-muted, #718096);
}

.rte__editor :deep(.rte-content) {
  min-height: 140px;
  padding: var(--space-2, 8px);
  font-size: 13px;
  line-height: 1.6;
  outline: none;
  background: var(--color-background, #fff);
  border: 1px solid var(--color-border, #d9d9e3);
  border-radius: var(--radius-sm, 4px);
}

.rte__preview {
  padding: var(--space-2, 8px);
  outline: none;
  background: var(--color-surface, #f7f7fa);
  border: 1px solid var(--color-border, #d9d9e3);
  border-radius: var(--radius-sm, 4px);
}

/* Shared typography for both the editor and the sanitised preview. */
.rte__editor :deep(.rte-content),
.rte__preview :deep(.rte-preview-body) {
  font-size: 13px;
  line-height: 1.6;
}

.rte__editor :deep(.rte-content h1),
.rte__preview :deep(.rte-preview-body h1) { font-size: 20px; margin: 8px 0 4px; }
.rte__editor :deep(.rte-content h2),
.rte__preview :deep(.rte-preview-body h2) { font-size: 17px; margin: 8px 0 4px; }
.rte__editor :deep(.rte-content h3),
.rte__preview :deep(.rte-preview-body h3) { font-size: 15px; margin: 6px 0 3px; }

.rte__editor :deep(.rte-content ul),
.rte__preview :deep(.rte-preview-body ul),
.rte__editor :deep(.rte-content ol),
.rte__preview :deep(.rte-preview-body ol) { padding-left: 20px; margin: 4px 0; }

.rte__editor :deep(.rte-content blockquote),
.rte__preview :deep(.rte-preview-body blockquote) {
  padding-left: 8px;
  margin: 4px 0;
  color: var(--color-text-muted, #718096);
  border-left: 3px solid var(--color-border, #d9d9e3);
}

.rte__editor :deep(.rte-content pre),
.rte__preview :deep(.rte-preview-body pre) {
  padding: 8px;
  overflow-x: auto;
  font-family: var(--font-mono, ui-monospace, monospace);
  font-size: 12px;
  background: var(--color-surface, #f7f7fa);
  border-radius: var(--radius-sm, 4px);
}

.rte__editor :deep(.rte-content code),
.rte__preview :deep(.rte-preview-body code) {
  font-family: var(--font-mono, ui-monospace, monospace);
  font-size: 12px;
}

.rte__editor :deep(.rte-content img),
.rte__preview :deep(.rte-preview-body img) { max-width: 100%; height: auto; }
</style>
