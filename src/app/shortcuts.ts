import {
  selectTask, toggleExpand, expandedTaskKeys, selectedTaskKey,
} from "../stores/app-state";
import { navigateVisibleRow } from "../stores/task-store";
import { commandPaletteOpen, closeCommandPalette, closeGlobalSearch, overlayOpen } from "../stores/ui-store";
import { settings } from "../stores/settings-store";
import { executeCommand } from "./commands";

type ShortcutHandler = (e: KeyboardEvent) => void;

interface ShortcutDef {
  key: string;
  ctrl?: boolean;
  shift?: boolean;
  alt?: boolean;
  /** Run even when focus is inside an input/textarea/contenteditable. */
  allowInInput?: boolean;
  /** Command id resolved through the registry (preferred over a raw handler). */
  command?: string;
  handler?: ShortcutHandler;
}

const shortcuts: ShortcutDef[] = [
  // ── Overlays ────────────────────────────────────────────────────────────────
  {
    key: "k",
    ctrl: true,
    allowInInput: true,
    handler: (e) => {
      e.preventDefault();
      if (!settings.commandPaletteEnabled) return;
      if (commandPaletteOpen.value) closeCommandPalette();
      else executeCommand("view.commandPalette");
    },
  },
  {
    key: "Escape",
    allowInInput: true,
    handler: (e) => {
      if (!overlayOpen()) return;
      e.preventDefault();
      closeCommandPalette();
      closeGlobalSearch();
    },
  },
  { key: "f", ctrl: true, allowInInput: true, command: "view.globalSearch" },
  { key: "Enter", ctrl: true, allowInInput: true, command: "task.quickAdd" },

  // ── Documents / editing ─────────────────────────────────────────────────────
  { key: "s", ctrl: true, allowInInput: true, command: "file.save" },
  { key: "s", ctrl: true, shift: true, allowInInput: true, command: "file.saveAll" },
  { key: "z", ctrl: true, allowInInput: true, command: "edit.undo" },
  { key: "y", ctrl: true, allowInInput: true, command: "edit.redo" },
  { key: "n", ctrl: true, allowInInput: true, command: "task.addRoot" },
  { key: "n", ctrl: true, shift: true, allowInInput: true, command: "task.addChild" },
  { key: "l", ctrl: true, allowInInput: true, command: "view.jumpToSelection" },

  // ── Tree navigation ─────────────────────────────────────────────────────────
  {
    key: "ArrowDown",
    allowInInput: false,
    handler: (e) => {
      e.preventDefault();
      const key = navigateVisibleRow(1);
      if (key) selectTask(key);
    },
  },
  {
    key: "ArrowUp",
    allowInInput: false,
    handler: (e) => {
      e.preventDefault();
      const key = navigateVisibleRow(-1);
      if (key) selectTask(key);
    },
  },
  {
    key: "ArrowRight",
    handler: (e) => {
      e.preventDefault();
      const key = selectedTaskKey.value;
      if (key && !expandedTaskKeys.value.has(key)) {
        toggleExpand(key);
      }
    },
  },
  {
    key: "ArrowLeft",
    handler: (e) => {
      e.preventDefault();
      const key = selectedTaskKey.value;
      if (key && expandedTaskKeys.value.has(key)) {
        toggleExpand(key);
      }
    },
  },
  {
    key: "Enter",
    handler: (e) => {
      e.preventDefault();
      const titleInput = document.querySelector<HTMLInputElement>(".title-editor__input");
      titleInput?.focus();
      titleInput?.select();
    },
  },
  { key: " ", command: "task.toggleComplete" },
  { key: "Delete", command: "task.delete" },
  { key: "F2", allowInInput: false, command: "view.jumpToSelection" },
];

function isInputFocused(): boolean {
  const el = document.activeElement;
  if (!el) return false;
  const tag = el.tagName.toLowerCase();
  return tag === "input" || tag === "textarea" || tag === "select"
    || (el as HTMLElement).isContentEditable;
}

function matches(def: ShortcutDef, e: KeyboardEvent): boolean {
  if (e.key !== def.key) return false;
  const ctrl = e.ctrlKey || e.metaKey;
  if (Boolean(def.ctrl) !== ctrl) return false;
  if (Boolean(def.shift) !== e.shiftKey) return false;
  if (def.alt && !e.altKey) return false;
  return true;
}

function run(def: ShortcutDef, e: KeyboardEvent): void {
  if (def.command) {
    executeCommand(def.command, { silentWhenDisabled: true });
    return;
  }
  def.handler?.(e);
}

let installed = false;

function onKeyDown(e: KeyboardEvent): void {
  // While an overlay owns the keyboard only overlay shortcuts may fire.
  const overlay = overlayOpen();

  for (const def of shortcuts) {
    if (!matches(def, e)) continue;
    const isOverlayShortcut = def.key === "Escape"
      || (def.ctrl === true && (def.key === "k" || def.key === "f"));
    if (overlay && !isOverlayShortcut) continue;
    if (!def.allowInInput && isInputFocused()) continue;
    run(def, e);
    return;
  }
}

export function initKeyboardShortcuts(): void {
  if (installed) return;
  installed = true;
  document.addEventListener("keydown", onKeyDown);
}

/** Test/teardown hook so the listener never outlives the app. */
export function disposeKeyboardShortcuts(): void {
  if (!installed) return;
  installed = false;
  document.removeEventListener("keydown", onKeyDown);
}
