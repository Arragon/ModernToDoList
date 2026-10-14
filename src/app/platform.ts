/**
 * Platform bridges that are not business commands: opening a URL in the
 * Windows default browser, revealing a path in Explorer, and picking a
 * directory/file.
 *
 * `tauri-plugin-shell` and `tauri-plugin-dialog` are registered in
 * `src-tauri/src/lib.rs`, with `shell:allow-open` and `dialog:default` granted in
 * `src-tauri/capabilities/default.json`. Pickers therefore open the native
 * Windows Explorer dialog via `plugin:dialog|open`; if that is ever unavailable
 * they fall back to the in-app path prompt (see `PathInputDialog.vue`). Every
 * route fails soft with a toast rather than throwing.
 */
import { invoke } from "@tauri-apps/api/core";
import { Commands } from "../ipc/commands";
import { describeIpcError, safeInvoke } from "../ipc/safe";
import { promptForPath, showToast } from "../stores/app-state";

const HTTP_SCHEME = /^https?:\/\//i;

/** Opens a URL (or local path) with the OS default handler. */
export async function openExternal(target: string): Promise<boolean> {
  const value = target.trim();
  if (!value) return false;
  if (!HTTP_SCHEME.test(value) && !/^[a-z]:[\\/]/i.test(value) && !value.startsWith("\\\\")) {
    showToast("Only http(s) URLs and local paths can be opened", "warning");
    return false;
  }

  try {
    await invoke(Commands.SHELL_OPEN, { path: value, with: null });
    return true;
  } catch (err) {
    const reason = describeIpcError(err);
    if (HTTP_SCHEME.test(value)) {
      // WebView2 fallback: a new window/tab is acceptable for http(s).
      const opened = window.open(value, "_blank", "noopener,noreferrer");
      if (opened) return true;
    }
    showToast(`Could not open "${value}" (${reason})`, "error");
    return false;
  }
}

/** Reveals a file/folder in Windows Explorer. */
export async function revealInExplorer(path: string): Promise<boolean> {
  const value = path.trim();
  if (!value) return false;

  // Prefer a dedicated backend command (it can resolve managed asset paths).
  const backend = await safeInvoke<null>("reveal_path", { path: value });
  if (backend.ok) return true;

  const directory = looksLikeDirectory(value) ? value : parentDirectory(value);
  try {
    await invoke(Commands.SHELL_OPEN, { path: directory, with: null });
    return true;
  } catch (err) {
    showToast(`Could not reveal "${value}" (${describeIpcError(err)})`, "error");
    return false;
  }
}

function looksLikeDirectory(path: string): boolean {
  return /[\\/]$/.test(path);
}

function parentDirectory(path: string): string {
  const index = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  return index > 0 ? path.slice(0, index) : path;
}

/** Extracts a Windows path from a `file://` URL produced by drag-and-drop. */
export function pathFromFileUrl(url: string): string | null {
  if (!url.startsWith("file://")) return null;
  try {
    const parsed = new URL(url);
    return decodeURIComponent(parsed.pathname).replace(/^\//, "");
  } catch {
    return decodeURIComponent(url.slice("file://".length)).replace(/^\//, "");
  }
}

export interface PickOptions {
  title: string;
  mode: "directory" | "file";
  placeholder?: string;
  message?: string;
  defaultValue?: string;
}

/**
 * Picks a directory or file.
 *
 * Tries the native dialog plugin first (so a future bundle of
 * `tauri-plugin-dialog` works without frontend changes) and otherwise opens the
 * in-app prompt. Resolves with `null` when cancelled.
 */
export async function pickPath(options: PickOptions): Promise<string | null> {
  const native = await tryNativeDialog(options);
  if (native !== undefined) return native;
  return promptForPath({
    title: options.title,
    message: options.message,
    placeholder: options.placeholder,
    mode: options.mode,
    defaultValue: options.defaultValue,
  });
}

/** `undefined` means "native dialog unavailable", `null` means "cancelled". */
async function tryNativeDialog(options: PickOptions): Promise<string | null | undefined> {
  const dialogOptions = options.mode === "directory"
    ? { directory: true, multiple: false, title: options.title, defaultPath: options.defaultValue }
    : { directory: false, multiple: false, title: options.title, defaultPath: options.defaultValue };

  const result = await safeInvoke<string | string[] | null>("plugin:dialog|open", {
    options: dialogOptions,
  });
  if (!result.ok) return undefined;
  const value = result.value;
  if (value === null || value === undefined) return null;
  return Array.isArray(value) ? value[0] ?? null : value;
}

export function fileNameOf(path: string): string {
  const index = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  return index >= 0 ? path.slice(index + 1) : path;
}

export function fileExtensionOf(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : "";
}

export function formatBytes(size: number | null): string {
  if (size === null || !Number.isFinite(size) || size < 0) return "—";
  if (size < 1024) return `${size} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = size / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${units[unit]}`;
}
