/**
 * User interface preferences: display language (zh / en) and color theme
 * (light / dark). Persisted to localStorage and applied to the document root so
 * the `[data-theme]` token overrides in tokens.css take effect app-wide.
 */
import { ref } from "vue";

export type Locale = "zh" | "en";
export type Theme = "light" | "dark";

const LOCALE_KEY = "mtl.locale";
const THEME_KEY = "mtl.theme";

function readStored<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return allowed.includes(raw as T) ? (raw as T) : fallback;
  } catch {
    return fallback;
  }
}

export const locale = ref<Locale>(readStored<Locale>(LOCALE_KEY, ["zh", "en"], "en"));
export const theme = ref<Theme>(readStored<Theme>(THEME_KEY, ["light", "dark"], "light"));

/** Pushes the current theme onto <html> so CSS token overrides apply. */
export function applyTheme(): void {
  document.documentElement.dataset.theme = theme.value;
}

export function setTheme(next: Theme): void {
  theme.value = next;
  try { localStorage.setItem(THEME_KEY, next); } catch { /* private mode */ }
  applyTheme();
}

export function toggleTheme(): void {
  setTheme(theme.value === "light" ? "dark" : "light");
}

export function setLocale(next: Locale): void {
  locale.value = next;
  try { localStorage.setItem(LOCALE_KEY, next); } catch { /* private mode */ }
}

export function toggleLocale(): void {
  setLocale(locale.value === "zh" ? "en" : "zh");
}

// Apply the persisted theme as early as the module loads.
applyTheme();
