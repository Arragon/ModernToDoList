/**
 * Quick Add grammar parser (RD-M9-035~041).
 *
 *   Task title #tag @participant !priority due:2024-01-15 start:2024-01-10 50%
 *
 * Rules:
 *  - `#tag`            → tag (spaces allowed inside `#{tag with space}`)
 *  - `@participant`    → participant (spaces allowed inside `@{Jane Doe}`)
 *  - `!priority`       → none|low|medium|med|high|veryhigh|urgent|0..4
 *  - `due:YYYY-MM-DD` / `start:YYYY-MM-DD` → dates (also `due:today|tomorrow|+3d`)
 *  - `50%`             → percent done
 *  - Anything unrecognised stays part of the title, in its original position
 *    (safe fallback — never lose user text).
 */

export interface QuickAddParseResult {
  title: string;
  tags: string[];
  participants: string[];
  /** -1 when the input did not specify a priority. */
  priority: number;
  startDate: string | null;
  dueDate: string | null;
  /** -1 when the input did not specify progress. */
  percentDone: number;
  /** Raw tokens that were not recognised (kept inside `title`). */
  unknownTokens: string[];
}

const PRIORITY_WORDS: Record<string, number> = {
  none: 0,
  low: 1,
  medium: 2,
  med: 2,
  normal: 2,
  high: 3,
  urgent: 4,
  veryhigh: 4,
  critical: 4,
};

const DATE_SHORTCUTS = new Set(["today", "tomorrow", "tmr", "now"]);

/** Splits on whitespace but keeps quoted / braced groups together. */
export function tokenize(input: string): string[] {
  const tokens: string[] = [];
  let current = "";
  let quote: string | null = null;
  let braceDepth = 0;

  const flush = () => {
    if (current.length > 0) {
      tokens.push(current);
      current = "";
    }
  };

  for (const char of input) {
    if (quote) {
      if (char === quote) {
        quote = null;
      } else {
        current += char;
      }
      continue;
    }
    if (char === '"' || char === "'") {
      quote = char;
      continue;
    }
    if (char === "{") {
      braceDepth += 1;
      current += char;
      continue;
    }
    if (char === "}") {
      braceDepth = Math.max(0, braceDepth - 1);
      current += char;
      continue;
    }
    if (braceDepth === 0 && /\s/.test(char)) {
      flush();
      continue;
    }
    current += char;
  }
  flush();
  return tokens;
}

function stripBraces(value: string): string {
  const first = value.indexOf("{");
  const last = value.lastIndexOf("}");
  if (first >= 0 && last > first) {
    return value.slice(first + 1, last).trim();
  }
  return value;
}

export function parsePriority(token: string): number | null {
  const raw = token.slice(1).toLowerCase();
  if (raw.length === 0) return null;
  if (PRIORITY_WORDS[raw] !== undefined) return PRIORITY_WORDS[raw];
  if (/^[0-4]$/.test(raw)) return Number.parseInt(raw, 10);
  return null;
}

export function parsePercent(token: string): number | null {
  if (!/^\d{1,3}%$/.test(token)) return null;
  const value = Number.parseInt(token.slice(0, -1), 10);
  return value >= 0 && value <= 100 ? value : null;
}

const ISO_DATE = /^\d{4}-\d{2}-\d{2}$/;
const RELATIVE_DAYS = /^([+-])(\d{1,3})d$/;

/** Normalises a date token value to ISO `YYYY-MM-DD`, or null when invalid. */
export function normaliseDate(value: string, today = new Date()): string | null {
  const lower = value.toLowerCase();
  if (ISO_DATE.test(lower)) return lower;

  const base = new Date(today.getFullYear(), today.getMonth(), today.getDate());
  if (lower === "today" || lower === "now") return toIso(base);
  if (lower === "tomorrow" || lower === "tmr") {
    base.setDate(base.getDate() + 1);
    return toIso(base);
  }
  const relative = RELATIVE_DAYS.exec(lower);
  if (relative) {
    const delta = Number.parseInt(relative[2], 10) * (relative[1] === "-" ? -1 : 1);
    base.setDate(base.getDate() + delta);
    return toIso(base);
  }
  if (DATE_SHORTCUTS.has(lower)) return toIso(base);
  return null;
}

function toIso(date: Date): string {
  const month = `${date.getMonth() + 1}`.padStart(2, "0");
  const day = `${date.getDate()}`.padStart(2, "0");
  return `${date.getFullYear()}-${month}-${day}`;
}

export function parseQuickAdd(input: string, today = new Date()): QuickAddParseResult {
  const tokens = tokenize(input);
  const titleParts: string[] = [];
  const unknownTokens: string[] = [];
  const tags: string[] = [];
  const participants: string[] = [];

  let priority = -1;
  let percentDone = -1;
  let startDate: string | null = null;
  let dueDate: string | null = null;

  for (const token of tokens) {
    if (token.startsWith("#") && token.length > 1) {
      const tag = stripBraces(token);
      if (tag) {
        tags.push(tag);
        continue;
      }
    }

    if (token.startsWith("@") && token.length > 1) {
      const name = stripBraces(token);
      if (name) {
        participants.push(name);
        continue;
      }
    }

    if (token.startsWith("!") && token.length > 1) {
      const parsed = parsePriority(token);
      if (parsed !== null) {
        priority = parsed;
        continue;
      }
    }

    const colon = token.indexOf(":");
    if (colon > 0) {
      const field = token.slice(0, colon).toLowerCase();
      const rawValue = token.slice(colon + 1);
      if (field === "due" || field === "start" || field === "starts") {
        const iso = normaliseDate(rawValue, today);
        if (iso) {
          if (field === "due") dueDate = iso;
          else startDate = iso;
          continue;
        }
      }
    }

    const percent = parsePercent(token);
    if (percent !== null) {
      percentDone = percent;
      continue;
    }

    // Safe fallback: unrecognised tokens remain part of the title.
    titleParts.push(token);
    unknownTokens.push(token);
  }

  return {
    title: titleParts.join(" ").trim(),
    tags: dedupe(tags),
    participants: dedupe(participants),
    priority,
    startDate,
    dueDate,
    percentDone,
    unknownTokens,
  };
}

function dedupe(values: string[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const value of values) {
    const key = value.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(value);
  }
  return out;
}
