/**
 * Runtime IPC capability tracking.
 *
 * Several backend commands are being implemented in parallel with the frontend.
 * When Tauri reports that a command is not registered we record it here so the
 * UI can degrade gracefully: controls bound to an unavailable command render
 * disabled and the failure is surfaced once as a toast instead of throwing an
 * unhandled rejection on every interaction.
 */
import { computed, ref } from "vue";
import type { ComputedRef } from "vue";

/** Commands known to be missing from the registered invoke handler. */
const unavailable = ref<ReadonlySet<string>>(new Set<string>());

/** Commands that failed for a reason other than "not registered". */
const failing = ref<ReadonlySet<string>>(new Set<string>());

export function isCommandAvailable(command: string): boolean {
  return !unavailable.value.has(command);
}

export function isCommandUsable(command: string): boolean {
  return !unavailable.value.has(command) && !failing.value.has(command);
}

export function markCommandUnavailable(command: string): void {
  if (unavailable.value.has(command)) return;
  const next = new Set(unavailable.value);
  next.add(command);
  unavailable.value = next;
}

export function markCommandFailing(command: string): void {
  if (failing.value.has(command)) return;
  const next = new Set(failing.value);
  next.add(command);
  failing.value = next;
}

/** Re-probe a command after the backend gained the implementation. */
export function resetCommandState(command: string): void {
  if (unavailable.value.has(command)) {
    const next = new Set(unavailable.value);
    next.delete(command);
    unavailable.value = next;
  }
  if (failing.value.has(command)) {
    const nextFailing = new Set(failing.value);
    nextFailing.delete(command);
    failing.value = nextFailing;
  }
}

export function resetAllCommandStates(): void {
  unavailable.value = new Set<string>();
  failing.value = new Set<string>();
}

export const unavailableCommands: ComputedRef<string[]> = computed(() =>
  Array.from(unavailable.value).sort(),
);

/** Reactive availability check for use inside templates / computed values. */
export function useCommandAvailable(command: string): ComputedRef<boolean> {
  return computed(() => isCommandAvailable(command));
}
