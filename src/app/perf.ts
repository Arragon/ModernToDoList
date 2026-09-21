/**
 * Frontend performance instrumentation (RD-M10-016 / RD-M10-022 evidence).
 *
 * Uses the Performance API (`performance.mark` / `performance.measure`) so the
 * samples are visible in the WebView2 / browser DevTools Performance panel as
 * well as in the in-app summary exposed on `window.__mtodoPerf`. The RD-M10-022
 * decision gate reads `summarize()` to compare tree render cost against the
 * 16ms frame budget without touching the source.
 */
import { readonly, ref } from "vue";
import type { DeepReadonly, Ref } from "vue";

export interface RenderSample {
  scope: string;
  /** Total rows the view could render. */
  rowCount: number;
  /** Rows actually mounted in the DOM. */
  renderedCount: number;
  virtualized: boolean;
  durationMs: number;
  timestamp: number;
}

export interface RenderSummary {
  scope: string;
  samples: number;
  rowCount: number;
  minMs: number;
  maxMs: number;
  avgMs: number;
  p95Ms: number;
  withinFrameBudget: boolean;
  virtualized: boolean;
}

export const FRAME_BUDGET_MS = 16;
const MAX_SAMPLES = 500;

const samples: Ref<RenderSample[]> = ref([]);
let markCounter = 0;

export interface RenderTimer {
  scope: string;
  mark: string;
  startedAt: number;
}

export function beginRender(scope: string): RenderTimer {
  const mark = `mtodo:${scope}:${++markCounter}`;
  if (typeof performance !== "undefined" && typeof performance.mark === "function") {
    try {
      performance.mark(mark);
    } catch {
      // Marks are best-effort; timing still works from the monotonic clock.
    }
  }
  return { scope, mark, startedAt: now() };
}

export function endRender(
  timer: RenderTimer,
  meta: { rowCount: number; renderedCount: number; virtualized: boolean },
): RenderSample {
  const durationMs = now() - timer.startedAt;
  if (typeof performance !== "undefined" && typeof performance.measure === "function") {
    try {
      performance.measure(`mtodo:${timer.scope}`, timer.mark);
      performance.clearMarks(timer.mark);
    } catch {
      // Nothing actionable: the numeric sample below is the source of truth.
    }
  }
  const sample: RenderSample = {
    scope: timer.scope,
    rowCount: meta.rowCount,
    renderedCount: meta.renderedCount,
    virtualized: meta.virtualized,
    durationMs,
    timestamp: Date.now(),
  };
  record(sample);
  return sample;
}

/** Record a sample produced outside a begin/end pair (e.g. scroll repaints). */
export function record(sample: RenderSample): void {
  const next = samples.value.concat(sample);
  samples.value = next.length > MAX_SAMPLES ? next.slice(next.length - MAX_SAMPLES) : next;
}

export function now(): number {
  return typeof performance !== "undefined" && typeof performance.now === "function"
    ? performance.now()
    : Date.now();
}

export const renderSamples: DeepReadonly<Ref<RenderSample[]>> = readonly(samples);

export function summarize(scope?: string): RenderSummary[] {
  const byScope = new Map<string, RenderSample[]>();
  for (const sample of samples.value) {
    if (scope && sample.scope !== scope) continue;
    const list = byScope.get(sample.scope) ?? [];
    list.push(sample);
    byScope.set(sample.scope, list);
  }

  const out: RenderSummary[] = [];
  for (const [name, list] of byScope) {
    const durations = list.map((s) => s.durationMs).sort((a, b) => a - b);
    const total = durations.reduce((acc, v) => acc + v, 0);
    const p95Index = Math.min(durations.length - 1, Math.floor(durations.length * 0.95));
    const avg = total / durations.length;
    out.push({
      scope: name,
      samples: list.length,
      rowCount: list[list.length - 1].rowCount,
      minMs: durations[0],
      maxMs: durations[durations.length - 1],
      avgMs: avg,
      p95Ms: durations[p95Index],
      withinFrameBudget: durations[p95Index] <= FRAME_BUDGET_MS,
      virtualized: list[list.length - 1].virtualized,
    });
  }
  return out.sort((a, b) => a.scope.localeCompare(b.scope));
}

export function clearSamples(): void {
  samples.value = [];
  if (typeof performance !== "undefined" && typeof performance.clearMarks === "function") {
    try {
      performance.clearMarks();
      performance.clearMeasures();
    } catch {
      // Ignore: instrumentation only.
    }
  }
}

export interface MtodoPerfApi {
  samples(): RenderSample[];
  summarize(scope?: string): RenderSummary[];
  clear(): void;
  frameBudgetMs: number;
}

declare global {
  interface Window {
    __mtodoPerf?: MtodoPerfApi;
  }
}

/** Publishes the measurement hooks for the RD-M10-022 gate / QA matrix H. */
export function installPerfBridge(): MtodoPerfApi {
  const api: MtodoPerfApi = {
    samples: () => samples.value.slice(),
    summarize,
    clear: clearSamples,
    frameBudgetMs: FRAME_BUDGET_MS,
  };
  if (typeof window !== "undefined") {
    window.__mtodoPerf = api;
  }
  return api;
}
