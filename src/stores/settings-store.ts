/**
 * Runtime UI settings.
 *
 * Everything here is disposable application state (never business data). It is
 * persisted to localStorage so a portable install keeps its layout between
 * runs, and every read is guarded so a blocked/corrupt store cannot break boot.
 *
 * Layout limits are read from the CSS design tokens in `styles/tokens.css`
 * instead of being duplicated here, so the tokens stay authoritative.
 */
import { computed, reactive, watch } from "vue";

const STORAGE_KEY = "mtodo.settings.v1";

export type VirtualizationMode = "auto" | "on" | "off";

export interface VirtualizationSettings {
  /** "on"/"off" force the behaviour, "auto" enables it above `threshold` rows. */
  mode: VirtualizationMode;
  threshold: number;
  overscan: number;
  /** Fallback row height used before a row has been measured. */
  rowHeight: number;
  /** Measure real row heights (variable) instead of assuming `rowHeight`. */
  dynamicHeights: boolean;
}

export interface LayoutSettings {
  sidebarWidth: number;
  inspectorWidth: number;
}

export interface AppSettings {
  layout: LayoutSettings;
  virtualization: VirtualizationSettings;
  quickAddEnabled: boolean;
  commandPaletteEnabled: boolean;
}

/** Reads a numeric design token (e.g. `--sidebar-min-width`) with a fallback. */
export function readTokenNumber(name: string, fallback: number): number {
  if (typeof window === "undefined" || typeof document === "undefined") return fallback;
  const raw = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  const parsed = Number.parseFloat(raw);
  return Number.isFinite(parsed) ? parsed : fallback;
}

export interface LayoutLimits {
  sidebarMin: number;
  sidebarMax: number;
  inspectorMin: number;
  inspectorMax: number;
}

export function layoutLimits(): LayoutLimits {
  return {
    sidebarMin: readTokenNumber("--sidebar-min-width", 180),
    sidebarMax: readTokenNumber("--sidebar-max-width", 400),
    inspectorMin: readTokenNumber("--inspector-min-width", 240),
    inspectorMax: readTokenNumber("--inspector-max-width", 500),
  };
}

function defaultSettings(): AppSettings {
  return {
    layout: {
      sidebarWidth: readTokenNumber("--sidebar-width", 240),
      inspectorWidth: readTokenNumber("--inspector-width", 320),
    },
    virtualization: {
      mode: "auto",
      threshold: 200,
      overscan: 8,
      rowHeight: 32,
      dynamicHeights: true,
    },
    quickAddEnabled: true,
    commandPaletteEnabled: true,
  };
}

function clampWidths(settings: AppSettings): AppSettings {
  const limits = layoutLimits();
  settings.layout.sidebarWidth = clamp(
    settings.layout.sidebarWidth, limits.sidebarMin, limits.sidebarMax,
  );
  settings.layout.inspectorWidth = clamp(
    settings.layout.inspectorWidth, limits.inspectorMin, limits.inspectorMax,
  );
  return settings;
}

export function clamp(value: number, min: number, max: number): number {
  if (!Number.isFinite(value)) return min;
  return Math.min(max, Math.max(min, value));
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function load(): AppSettings {
  const base = defaultSettings();
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return base;
    const parsed: unknown = JSON.parse(raw);
    if (!isRecord(parsed)) return base;
    if (isRecord(parsed.layout)) {
      const layout = parsed.layout;
      if (typeof layout.sidebarWidth === "number") base.layout.sidebarWidth = layout.sidebarWidth;
      if (typeof layout.inspectorWidth === "number") base.layout.inspectorWidth = layout.inspectorWidth;
    }
    if (isRecord(parsed.virtualization)) {
      const v = parsed.virtualization;
      if (v.mode === "auto" || v.mode === "on" || v.mode === "off") base.virtualization.mode = v.mode;
      if (typeof v.threshold === "number") base.virtualization.threshold = Math.max(1, v.threshold);
      if (typeof v.overscan === "number") base.virtualization.overscan = Math.max(0, v.overscan);
      if (typeof v.rowHeight === "number") base.virtualization.rowHeight = Math.max(8, v.rowHeight);
      if (typeof v.dynamicHeights === "boolean") base.virtualization.dynamicHeights = v.dynamicHeights;
    }
    if (typeof parsed.quickAddEnabled === "boolean") base.quickAddEnabled = parsed.quickAddEnabled;
    if (typeof parsed.commandPaletteEnabled === "boolean") {
      base.commandPaletteEnabled = parsed.commandPaletteEnabled;
    }
  } catch {
    // Corrupt or unavailable storage: fall back to defaults.
  }
  return clampWidths(base);
}

export const settings = reactive<AppSettings>(load());

watch(
  settings,
  (value) => {
    try {
      window.localStorage.setItem(STORAGE_KEY, JSON.stringify(value));
    } catch {
      // Non-fatal: settings simply will not persist this run.
    }
  },
  { deep: true },
);

export const sidebarWidth = computed({
  get: () => settings.layout.sidebarWidth,
  set: (value: number) => {
    const limits = layoutLimits();
    settings.layout.sidebarWidth = clamp(value, limits.sidebarMin, limits.sidebarMax);
  },
});

export const inspectorWidth = computed({
  get: () => settings.layout.inspectorWidth,
  set: (value: number) => {
    const limits = layoutLimits();
    settings.layout.inspectorWidth = clamp(value, limits.inspectorMin, limits.inspectorMax);
  },
});

/**
 * Effective virtualization decision for a given row count.
 * RD-M10-022 can flip `mode` at runtime (settings UI, command palette or
 * localStorage) without a code change to enable or skip RD-M10-023.
 */
export function virtualizationActive(rowCount: number): boolean {
  const v = settings.virtualization;
  if (v.mode === "on") return true;
  if (v.mode === "off") return false;
  return rowCount > v.threshold;
}

export function resetSettings(): void {
  const fresh = defaultSettings();
  settings.layout.sidebarWidth = fresh.layout.sidebarWidth;
  settings.layout.inspectorWidth = fresh.layout.inspectorWidth;
  settings.virtualization.mode = fresh.virtualization.mode;
  settings.virtualization.threshold = fresh.virtualization.threshold;
  settings.virtualization.overscan = fresh.virtualization.overscan;
  settings.virtualization.rowHeight = fresh.virtualization.rowHeight;
  settings.virtualization.dynamicHeights = fresh.virtualization.dynamicHeights;
  settings.quickAddEnabled = fresh.quickAddEnabled;
  settings.commandPaletteEnabled = fresh.commandPaletteEnabled;
}
