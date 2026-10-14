/**
 * Lightweight fuzzy matcher used by the Ctrl+K command palette and the
 * dependency/search pickers. No external dependency (RD-M9-031~034).
 */

export interface FuzzyMatch {
  matched: boolean;
  score: number;
  /** Indices in `text` that matched, for highlight rendering. */
  indices: number[];
}

const NO_MATCH: FuzzyMatch = { matched: false, score: -1, indices: [] };

/**
 * Subsequence match with bonuses:
 *  - exact substring  → strong bonus (keeps prefix search natural)
 *  - word-boundary hit → medium bonus (camelCase / snake_case / spaces)
 *  - consecutive hits → small accumulating bonus
 * Lower scores are better; `matched === false` when not a subsequence.
 */
export function fuzzyScore(query: string, text: string): FuzzyMatch {
  const q = query.trim().toLowerCase();
  if (q.length === 0) return { matched: true, score: 0, indices: [] };
  const t = text.toLowerCase();
  if (t.length === 0) return NO_MATCH;

  const exact = t.indexOf(q);
  if (exact >= 0) {
    const indices: number[] = [];
    for (let i = 0; i < q.length; i += 1) indices.push(exact + i);
    return { matched: true, score: exact - 1000, indices };
  }

  let qi = 0;
  let score = 0;
  let streak = 0;
  const indices: number[] = [];

  for (let ti = 0; ti < t.length && qi < q.length; ti += 1) {
    if (t[ti] !== q[qi]) continue;
    indices.push(ti);
    const atBoundary = ti === 0 || /[\s_\-./\\()[\]#:]/.test(t[ti - 1]);
    score += atBoundary ? -12 : -2;
    streak += 1;
    score -= Math.min(streak, 6);
    qi += 1;
  }

  if (qi < q.length) return NO_MATCH;
  // Prefer short texts and early first hits.
  score += t.length * 0.05;
  score += indices[0] * 0.5;
  return { matched: true, score, indices };
}

export interface Ranked<T> {
  item: T;
  score: number;
  indices: number[];
}

/** Ranks `items` by fuzzy score against `query`, best first. */
export function fuzzyRank<T>(
  items: readonly T[],
  query: string,
  getText: (item: T) => string,
  getSecondary?: (item: T) => string,
  limit?: number,
): Ranked<T>[] {
  const out: Ranked<T>[] = [];
  for (const item of items) {
    const primary = fuzzyScore(query, getText(item));
    if (!primary.matched) {
      if (!getSecondary) continue;
      const secondary = fuzzyScore(query, getSecondary(item));
      if (!secondary.matched) continue;
      out.push({ item, score: secondary.score + 40, indices: [] });
      continue;
    }
    out.push({ item, score: primary.score, indices: primary.indices });
  }
  out.sort((a, b) => a.score - b.score);
  return limit ? out.slice(0, limit) : out;
}

/** True when every whitespace-separated term appears in `text` (order-free). */
export function includesAllTerms(query: string, text: string): boolean {
  const terms = query.toLowerCase().split(/\s+/).filter(Boolean);
  const haystack = text.toLowerCase();
  return terms.every((term) => haystack.includes(term));
}
