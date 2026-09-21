import { workspace, selectedTaskKey, closeWorkspace, scanAndIndex, rebuildIndex } from "../stores/app-state";

export interface AppCommand {
  id: string;
  label: string;
  icon?: string;
  shortcut?: string;
  enabled: () => boolean;
  execute: () => void | Promise<void>;
}

const commands = new Map<string, AppCommand>();

export function registerCommand(cmd: AppCommand) {
  commands.set(cmd.id, cmd);
}

export function getCommand(id: string): AppCommand | undefined {
  return commands.get(id);
}

export function executeCommand(id: string) {
  const cmd = commands.get(id);
  if (cmd && cmd.enabled()) {
    cmd.execute();
  }
}

export function getAllCommands(): AppCommand[] {
  return Array.from(commands.values());
}

// Register default commands
registerCommand({
  id: "workspace.open",
  label: "Open Workspace",
  icon: "fa-folder-open",
  enabled: () => true,
  execute: () => {},
});

registerCommand({
  id: "workspace.close",
  label: "Close Workspace",
  icon: "fa-folder-minus",
  enabled: () => workspace.value !== null,
  execute: () => closeWorkspace(),
});

registerCommand({
  id: "workspace.scanAndIndex",
  label: "Scan & Index",
  icon: "fa-magnifying-glass",
  enabled: () => workspace.value !== null,
  execute: () => scanAndIndex(),
});

registerCommand({
  id: "workspace.rebuildIndex",
  label: "Rebuild Index",
  icon: "fa-arrows-rotate",
  enabled: () => workspace.value !== null,
  execute: () => rebuildIndex(),
});

registerCommand({
  id: "task.addRoot",
  label: "Add Root Task",
  icon: "fa-plus",
  shortcut: "Ctrl+N",
  enabled: () => workspace.value !== null,
  execute: () => {},
});

registerCommand({
  id: "task.delete",
  label: "Delete Task",
  icon: "fa-trash",
  shortcut: "Delete",
  enabled: () => workspace.value !== null && selectedTaskKey.value !== null,
  execute: () => {},
});

registerCommand({
  id: "edit.undo",
  label: "Undo",
  icon: "fa-rotate-left",
  shortcut: "Ctrl+Z",
  enabled: () => workspace.value !== null,
  execute: () => {},
});

registerCommand({
  id: "edit.redo",
  label: "Redo",
  icon: "fa-rotate-right",
  shortcut: "Ctrl+Y",
  enabled: () => workspace.value !== null,
  execute: () => {},
});

registerCommand({
  id: "file.save",
  label: "Save",
  icon: "fa-floppy-disk",
  shortcut: "Ctrl+S",
  enabled: () => workspace.value !== null,
  execute: () => {},
});
