/**
 * Transient UI overlay state (command palette, global search, quick add focus).
 * Kept separate from `app-state` so overlay toggling never re-renders the
 * workspace/document parts of the tree.
 */
import { ref } from "vue";

export const commandPaletteOpen = ref(false);
export const globalSearchOpen = ref(false);
export const quickAddFocusRequest = ref(0);

export function openCommandPalette(): void {
  globalSearchOpen.value = false;
  commandPaletteOpen.value = true;
}

export function closeCommandPalette(): void {
  commandPaletteOpen.value = false;
}

export function toggleCommandPalette(): void {
  if (commandPaletteOpen.value) closeCommandPalette();
  else openCommandPalette();
}

export function openGlobalSearch(): void {
  commandPaletteOpen.value = false;
  globalSearchOpen.value = true;
}

export function closeGlobalSearch(): void {
  globalSearchOpen.value = false;
}

export function toggleGlobalSearch(): void {
  if (globalSearchOpen.value) closeGlobalSearch();
  else openGlobalSearch();
}

/** Asks the Quick Add input to take focus (bumped counter = one-shot signal). */
export function requestQuickAddFocus(): void {
  quickAddFocusRequest.value += 1;
}

/** True while any modal overlay owns the keyboard. */
export function overlayOpen(): boolean {
  return commandPaletteOpen.value || globalSearchOpen.value;
}
