<script setup lang="ts">
/**
 * Rich-text formatting toolbar (RD-M7-003~010).
 *
 * Only the approved control set is exposed. TextAlign and Highlight are
 * registered in `extensions.ts` for paste tolerance but deliberately have no
 * button here, and there is no image control because inserting images belongs
 * to the M7 managed-asset pipeline rather than to formatting.
 *
 * Every control produces markup the backend whitelist sanitizer keeps, so a
 * user can never apply formatting that silently disappears on save.
 */
import { onBeforeUnmount, onMounted, ref } from "vue";
import type { Editor } from "@tiptap/vue-3";
import type { ChainedCommands } from "@tiptap/core";
import { FONT_SIZES, TEXT_COLORS } from "./extensions";

const props = defineProps<{ editor: Editor | undefined; disabled?: boolean }>();
const emit = defineEmits<{ (e: "request-link"): void }>();

// Tiptap mutates the editor in place, so button state would not re-render on
// selection changes. Bumping a counter on every transaction makes the `isActive`
// reads below reactive without depending on Tiptap's internals.
const tick = ref(0);
let bound: Editor | undefined;

function attach(editor: Editor | undefined): void {
  if (bound === editor) return;
  if (bound) bound.off("transaction", onTransaction);
  bound = editor;
  if (bound) bound.on("transaction", onTransaction);
}
function onTransaction(): void {
  tick.value += 1;
}

onMounted(() => attach(props.editor));
onBeforeUnmount(() => {
  if (bound) bound.off("transaction", onTransaction);
  bound = undefined;
});

// Re-bind whenever the editor instance appears or is replaced.
watchEditor();
function watchEditor(): void {
  // `props.editor` starts undefined and is assigned once useEditor resolves.
  const poll = window.setInterval(() => {
    if (props.editor) {
      attach(props.editor);
      window.clearInterval(poll);
    }
  }, 50);
  onBeforeUnmount(() => window.clearInterval(poll));
}

function active(name: string, attrs?: Record<string, unknown>): boolean {
  void tick.value;
  return props.editor?.isActive(name, attrs as never) ?? false;
}

function currentFontSize(): string {
  void tick.value;
  const size = props.editor?.getAttributes("textStyle")?.fontSize;
  return typeof size === "string" ? size : "";
}

function currentColor(): string {
  void tick.value;
  const color = props.editor?.getAttributes("textStyle")?.color;
  return typeof color === "string" ? color : "";
}

function run(command: (chain: ChainedCommands) => ChainedCommands): void {
  const editor = props.editor;
  if (!editor || props.disabled) return;
  command(editor.chain().focus()).run();
}

function applyFontSize(size: string): void {
  const editor = props.editor;
  if (!editor || props.disabled) return;
  if (size === "") editor.chain().focus().unsetFontSize().run();
  else editor.chain().focus().setFontSize(size).run();
}

function applyColor(color: string): void {
  const editor = props.editor;
  if (!editor || props.disabled) return;
  if (color === "") editor.chain().focus().unsetColor().run();
  else editor.chain().focus().setColor(color).run();
}
</script>

<template>
  <div class="rte-toolbar" role="toolbar" aria-label="Formatting" :aria-disabled="disabled">
    <div class="rte-toolbar__group">
      <button type="button" class="rte-btn" :class="{ 'is-active': active('bold') }" :disabled="disabled"
        title="Bold (Ctrl+B)" aria-label="Bold" @click="run((c) => c.toggleBold())">B</button>
      <button type="button" class="rte-btn rte-btn--italic" :class="{ 'is-active': active('italic') }" :disabled="disabled"
        title="Italic (Ctrl+I)" aria-label="Italic" @click="run((c) => c.toggleItalic())">I</button>
      <button type="button" class="rte-btn rte-btn--underline" :class="{ 'is-active': active('underline') }" :disabled="disabled"
        title="Underline (Ctrl+U)" aria-label="Underline" @click="run((c) => c.toggleUnderline())">U</button>
      <button type="button" class="rte-btn rte-btn--strike" :class="{ 'is-active': active('strike') }" :disabled="disabled"
        title="Strikethrough (Ctrl+Shift+S)" aria-label="Strikethrough" @click="run((c) => c.toggleStrike())">S</button>
    </div>

    <div class="rte-toolbar__group">
      <button v-for="level in [1, 2, 3]" :key="level" type="button" class="rte-btn"
        :class="{ 'is-active': active('heading', { level }) }" :disabled="disabled"
        :title="`Heading ${level} (Ctrl+Alt+${level})`" :aria-label="`Heading ${level}`"
        @click="run((c) => c.toggleHeading({ level: level as 1 | 2 | 3 }))">H{{ level }}</button>
      <select class="rte-select" :value="currentFontSize()" :disabled="disabled" aria-label="Font size"
        @change="applyFontSize(($event.target as HTMLSelectElement).value)">
        <option value="">Size</option>
        <option v-for="size in FONT_SIZES" :key="size" :value="size">{{ size }}</option>
      </select>
    </div>

    <div class="rte-toolbar__group rte-toolbar__group--colors">
      <button type="button" class="rte-btn rte-btn--color-reset" :disabled="disabled" title="Clear colour"
        aria-label="Clear text colour" @click="applyColor('')">&times;</button>
      <button v-for="swatch in TEXT_COLORS" :key="swatch.value" type="button" class="rte-swatch"
        :class="{ 'is-active': currentColor().toLowerCase() === swatch.value.toLowerCase() }" :disabled="disabled"
        :style="{ backgroundColor: swatch.value }" :title="swatch.label" :aria-label="`Text colour ${swatch.label}`"
        @click="applyColor(swatch.value)" />
    </div>

    <div class="rte-toolbar__group">
      <button type="button" class="rte-btn" :class="{ 'is-active': active('bulletList') }" :disabled="disabled"
        title="Bullet list (Ctrl+Shift+8)" aria-label="Bullet list" @click="run((c) => c.toggleBulletList())">&#8226;</button>
      <button type="button" class="rte-btn" :class="{ 'is-active': active('orderedList') }" :disabled="disabled"
        title="Numbered list (Ctrl+Shift+7)" aria-label="Numbered list" @click="run((c) => c.toggleOrderedList())">1.</button>
      <button type="button" class="rte-btn" :class="{ 'is-active': active('blockquote') }" :disabled="disabled"
        title="Blockquote" aria-label="Blockquote" @click="run((c) => c.toggleBlockquote())">&#10077;</button>
    </div>

    <div class="rte-toolbar__group">
      <button type="button" class="rte-btn" :class="{ 'is-active': active('link') }" :disabled="disabled"
        title="Link (Ctrl+K)" aria-label="Insert link" @click="emit('request-link')">&#128279;</button>
      <button type="button" class="rte-btn rte-btn--mono" :class="{ 'is-active': active('code') }" :disabled="disabled"
        title="Inline code (Ctrl+E)" aria-label="Inline code" @click="run((c) => c.toggleCode())">&lt;/&gt;</button>
      <button type="button" class="rte-btn rte-btn--mono" :class="{ 'is-active': active('codeBlock') }" :disabled="disabled"
        title="Code block (Ctrl+Shift+C)" aria-label="Code block" @click="run((c) => c.toggleCodeBlock())">{ }</button>
    </div>
  </div>
</template>

<style scoped>
.rte-toolbar {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2, 8px);
  align-items: center;
  padding: var(--space-1, 4px) var(--space-2, 8px);
  border: 1px solid var(--color-border, #d9d9e3);
  border-radius: var(--radius-sm, 4px);
  background: var(--color-surface, #f7f7fa);
}

.rte-toolbar__group {
  display: flex;
  gap: 2px;
  align-items: center;
}

.rte-toolbar__group + .rte-toolbar__group {
  padding-left: var(--space-2, 8px);
  border-left: 1px solid var(--color-border, #d9d9e3);
}

.rte-btn {
  min-width: 28px;
  height: 26px;
  padding: 0 6px;
  font: inherit;
  font-size: 12px;
  color: var(--color-text-primary, #1a1a2e);
  cursor: pointer;
  background: transparent;
  border: 1px solid transparent;
  border-radius: var(--radius-sm, 4px);
}

.rte-btn:hover:not(:disabled) {
  background: var(--color-hover, #ececf3);
}

.rte-btn:disabled {
  cursor: default;
  opacity: 0.45;
}

.rte-btn.is-active {
  color: var(--color-accent, #5a67d8);
  background: var(--color-selected, #e6e8fb);
  border-color: var(--color-accent, #5a67d8);
}

.rte-btn--italic { font-style: italic; }
.rte-btn--underline { text-decoration: underline; }
.rte-btn--strike { text-decoration: line-through; }
.rte-btn--mono { font-family: var(--font-mono, ui-monospace, monospace); }

.rte-select {
  height: 26px;
  padding: 0 4px;
  font: inherit;
  font-size: 12px;
  background: var(--color-background, #fff);
  border: 1px solid var(--color-border, #d9d9e3);
  border-radius: var(--radius-sm, 4px);
}

.rte-swatch {
  width: 18px;
  height: 18px;
  padding: 0;
  cursor: pointer;
  border: 1px solid var(--color-border, #d9d9e3);
  border-radius: 50%;
}

.rte-swatch.is-active {
  box-shadow: 0 0 0 2px var(--color-accent, #5a67d8);
}

.rte-swatch:disabled {
  cursor: default;
  opacity: 0.45;
}

.rte-btn--color-reset {
  font-size: 14px;
  line-height: 1;
}
</style>
