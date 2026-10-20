<script setup lang="ts">
/**
 * Global Quick Add shortcut settings (RD-M9-042~043).
 *
 * The backend owns registration, OS-level conflict detection and persistence
 * (`set_global_quick_add_shortcut` / `get_global_shortcut_settings` /
 * `get_shortcut_conflicts`); this section is a thin, fail-soft surface over
 * those commands. Every failure degrades to a status line instead of an
 * error dialog, matching the backend's "toggle never hard-errors" contract.
 */
import { onMounted, ref } from "vue";
import * as ipc from "../../ipc/client";
import type { GlobalShortcutSettings, ShortcutConflict } from "../../ipc/types";
import { promptForText, showToast } from "../../stores/app-state";

const settings = ref<GlobalShortcutSettings | null>(null);
const conflicts = ref<ShortcutConflict[]>([]);
const busy = ref(false);

const backendAvailable = ref(true);

onMounted(() => {
  void refresh();
});

async function refresh(): Promise<void> {
  const [s, c] = await Promise.all([
    ipc.getGlobalShortcutSettings(),
    ipc.getShortcutConflicts(),
  ]);
  if (s.ok) {
    settings.value = s.value;
  } else {
    backendAvailable.value = !s.missing;
  }
  if (c.ok) conflicts.value = c.value;
}

function statusText(): string {
  const s = settings.value;
  if (!s) return "";
  const last = s.last_status;
  if (!last) return s.quick_add_enabled ? "Registered" : "Off";
  switch (last.status) {
    case "registered":
      return "Registered with the OS";
    case "conflict":
      return `Conflict: held by ${last.holder}`;
    case "unavailable":
      return `OS registration unavailable (${last.reason}); in-app shortcut still works`;
    case "disabled":
      return "Off";
  }
}

function statusTone(): string {
  const last = settings.value?.last_status;
  if (!last) return "ok";
  if (last.status === "conflict" || last.status === "unavailable") return "warn";
  if (last.status === "registered") return "ok";
  return "muted";
}

async function toggle(): Promise<void> {
  if (!settings.value || busy.value) return;
  busy.value = true;
  try {
    const next = !settings.value.quick_add_enabled;
    const result = await ipc.setGlobalQuickAddShortcut(next);
    if (result.ok) {
      settings.value = result.value;
    } else {
      showToast(`Could not change the global shortcut: ${result.error}`, "warning");
    }
  } finally {
    busy.value = false;
  }
}

async function editAccelerator(): Promise<void> {
  if (!settings.value || busy.value) return;
  const value = await promptForText({
    title: "Global Quick Add shortcut",
    message: "Accelerator (for example Ctrl+Shift+Q). Leave empty to keep the current one.",
    defaultValue: settings.value.quick_add_accelerator,
    confirmLabel: "Save",
  });
  if (value === null) return;
  const accelerator = value.trim();
  busy.value = true;
  try {
    const result = await ipc.setGlobalQuickAddShortcut(true, accelerator || undefined);
    if (result.ok) {
      settings.value = result.value;
    } else {
      showToast(`Could not register the shortcut: ${result.error}`, "warning");
    }
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <div v-if="settings" class="sidebar__section">
    <div class="sidebar__section-title">Quick Add Hotkey</div>
    <label class="shortcut-settings__row">
      <input
        type="checkbox"
        :checked="settings.quick_add_enabled"
        :disabled="busy"
        @change="toggle"
      />
      <span>Global shortcut</span>
    </label>
    <div class="sidebar__item">
      <span class="sidebar__label shortcut-settings__accel" :title="'Change accelerator'">
        {{ settings.quick_add_accelerator }}
      </span>
      <button class="shortcut-settings__edit" title="Change accelerator" @click="editAccelerator">
        <i class="fas fa-pen"></i>
      </button>
    </div>
    <div class="sidebar__item">
      <span :class="['status-dot', `status-dot--${statusTone()}`]"></span>
      <span class="sidebar__label">{{ statusText() }}</span>
    </div>
    <div v-if="conflicts.length > 0" class="shortcut-settings__conflicts">
      <i class="fas fa-triangle-exclamation"></i>
      <span>
        {{ conflicts.length }} shortcut conflict(s) in the command registry:
        {{ conflicts.map((c) => c.shortcut).join(", ") }}
      </span>
    </div>
    <div v-else-if="!backendAvailable" class="sidebar__hint">
      Shortcut settings unavailable (backend command missing)
    </div>
  </div>
</template>

<style scoped>
.shortcut-settings__row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-sm);
  color: var(--color-text-secondary);
  padding: var(--space-1) var(--space-2);
  cursor: pointer;
}
.shortcut-settings__accel {
  font-family: var(--font-mono, monospace);
  flex: 1;
}
.shortcut-settings__edit {
  background: none;
  border: none;
  cursor: pointer;
  color: var(--color-text-muted);
  font-size: var(--text-xs);
  padding: 0 2px;
}
.shortcut-settings__edit:hover { color: var(--color-accent); }
.shortcut-settings__conflicts {
  margin-top: var(--space-2);
  font-size: var(--text-xs);
  color: var(--color-warning);
  display: flex;
  align-items: flex-start;
  gap: var(--space-1);
}
.status-dot--muted { background: var(--color-text-muted); }
</style>
