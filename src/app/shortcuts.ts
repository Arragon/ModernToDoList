import { selectTask, toggleExpand, expandedTaskKeys, selectedTaskKey } from "../stores/app-state";
import { navigateVisibleRow } from "../stores/task-store";
import { executeCommand } from "./commands";

type ShortcutHandler = (e: KeyboardEvent) => void;

interface ShortcutDef {
  key: string;
  ctrl?: boolean;
  shift?: boolean;
  handler: ShortcutHandler;
  skipInInput?: boolean;
}

const shortcuts: ShortcutDef[] = [
  {
    key: "ArrowDown",
    handler: (e) => {
      e.preventDefault();
      const key = navigateVisibleRow(1);
      if (key) selectTask(key);
    },
    skipInInput: false,
  },
  {
    key: "ArrowUp",
    handler: (e) => {
      e.preventDefault();
      const key = navigateVisibleRow(-1);
      if (key) selectTask(key);
    },
    skipInInput: false,
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
    skipInInput: true,
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
    skipInInput: true,
  },
  {
    key: "Enter",
    handler: (e) => {
      e.preventDefault();
      const titleInput = document.querySelector(".title-editor__input") as HTMLInputElement | null;
      if (titleInput) titleInput.focus();
    },
    skipInInput: true,
  },
  {
    key: " ",
    handler: () => {
      // Space toggles completion — would call Core via IPC
    },
    skipInInput: true,
  },
  {
    key: "Delete",
    handler: () => {
      executeCommand("task.delete");
    },
    skipInInput: true,
  },
  {
    key: "z",
    ctrl: true,
    handler: (e) => {
      e.preventDefault();
      executeCommand("edit.undo");
    },
    skipInInput: false,
  },
  {
    key: "y",
    ctrl: true,
    handler: (e) => {
      e.preventDefault();
      executeCommand("edit.redo");
    },
    skipInInput: false,
  },
  {
    key: "s",
    ctrl: true,
    handler: (e) => {
      e.preventDefault();
      executeCommand("file.save");
    },
    skipInInput: false,
  },
  {
    key: "n",
    ctrl: true,
    handler: (e) => {
      e.preventDefault();
      executeCommand("task.addRoot");
    },
    skipInInput: false,
  },
];

function isInputFocused(): boolean {
  const el = document.activeElement;
  if (!el) return false;
  const tag = el.tagName.toLowerCase();
  return tag === "input" || tag === "textarea" || tag === "select" || (el as HTMLElement).isContentEditable;
}

export function initKeyboardShortcuts() {
  document.addEventListener("keydown", (e: KeyboardEvent) => {
    for (const sc of shortcuts) {
      const ctrlMatch = sc.ctrl ? (e.ctrlKey || e.metaKey) : !(e.ctrlKey || e.metaKey || e.altKey);
      const shiftMatch = sc.shift ? e.shiftKey : !e.shiftKey;
      if (e.key === sc.key && ctrlMatch && shiftMatch) {
        if (sc.skipInInput && isInputFocused()) continue;
        sc.handler(e);
        return;
      }
    }
  });
}
