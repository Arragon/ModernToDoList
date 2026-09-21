/**
 * Global search (RD-M9-008~013).
 *
 * Prefers the backend FTS5 service (`global_search`, including the CJK LIKE
 * fallback that lives server-side). When that command is unavailable the store
 * degrades to an in-memory fuzzy search over the loaded task index so the UI
 * stays usable, and reports which source produced the hits.
 */
import { computed, ref } from "vue";
import type { SearchHitDto } from "../ipc/types";
import * as ipc from "../ipc/client";
import { markCommandUnavailable } from "./capability-store";
import { Commands } from "../ipc/commands";
import { fuzzyRank, includesAllTerms } from "../app/fuzzy";
import { allTasks, jumpToTask, tagsFor } from "./task-store";
import { participantsFor } from "./relation-store";

export type SearchSource = "backend" | "local" | "none";

export interface SearchHit {
  taskKey: string;
  documentId: string;
  title: string;
  documentPath: string | null;
  snippet: string;
  matchedField: string;
  score: number;
}

export const searchQuery = ref("");
export const searchHits = ref<SearchHit[]>([]);
export const searching = ref(false);
export const searchSource = ref<SearchSource>("none");
export const searchTruncated = ref(false);
export const searchTotal = ref(0);

export const hasResults = computed(() => searchHits.value.length > 0);

let requestToken = 0;

export async function runSearch(query: string, limit = 50): Promise<SearchHit[]> {
  const trimmed = query.trim();
  const token = ++requestToken;
  searchQuery.value = query;

  if (!trimmed) {
    searchHits.value = [];
    searchSource.value = "none";
    searchTotal.value = 0;
    searchTruncated.value = false;
    return [];
  }

  searching.value = true;
  try {
    const remote = await ipc.globalSearch(trimmed, undefined, limit);
    if (token !== requestToken) return [];

    if (remote.ok) {
      const hits = remote.value.hits.map(fromDto);
      searchHits.value = hits;
      searchSource.value = "backend";
      searchTotal.value = remote.value.total;
      searchTruncated.value = remote.value.truncated;
      return hits;
    }
    if (remote.missing) markCommandUnavailable(Commands.GLOBAL_SEARCH);

    const local = localSearch(trimmed, limit);
    if (token !== requestToken) return [];
    searchHits.value = local;
    searchSource.value = "local";
    searchTotal.value = local.length;
    searchTruncated.value = allTasks.value.length > local.length;
    return local;
  } finally {
    if (token === requestToken) searching.value = false;
  }
}

function fromDto(dto: SearchHitDto): SearchHit {
  return {
    taskKey: dto.task_key,
    documentId: dto.document_id,
    title: dto.title,
    documentPath: dto.document_path,
    snippet: dto.snippet,
    matchedField: dto.matched_field,
    score: dto.score,
  };
}

/**
 * In-memory fallback: substring match over title, tags and participant names
 * (CJK-safe because it is a plain `includes`), ranked by fuzzy score.
 */
export function localSearch(query: string, limit = 50): SearchHit[] {
  const tasks = allTasks.value;
  const candidates = tasks.filter((t) => {
    if (includesAllTerms(query, t.title)) return true;
    if (tagsFor(t.task_key).some((tag) => includesAllTerms(query, tag))) return true;
    return participantsFor(t.task_key).some((p) => includesAllTerms(query, p.display_name));
  });

  const ranked = fuzzyRank(
    candidates,
    query,
    (t) => t.title,
    (t) => [...tagsFor(t.task_key), ...participantsFor(t.task_key).map((p) => p.display_name)].join(" "),
    limit,
  );

  return ranked.map((entry) => ({
    taskKey: entry.item.task_key,
    documentId: entry.item.document_id,
    title: entry.item.title,
    documentPath: null,
    snippet: buildSnippet(entry.item.title, query),
    matchedField: "title",
    score: -entry.score,
  }));
}

function buildSnippet(title: string, query: string): string {
  const index = title.toLowerCase().indexOf(query.toLowerCase().split(/\s+/)[0] ?? "");
  if (index < 0) return title;
  const start = Math.max(0, index - 24);
  const end = Math.min(title.length, index + query.length + 24);
  return `${start > 0 ? "…" : ""}${title.slice(start, end)}${end < title.length ? "…" : ""}`;
}

export function clearSearch(): void {
  requestToken += 1;
  searchQuery.value = "";
  searchHits.value = [];
  searchSource.value = "none";
  searchTotal.value = 0;
  searchTruncated.value = false;
  searching.value = false;
}

/** Selects the task and asks the tree to scroll it into view. */
export function jumpToHit(hit: SearchHit): boolean {
  return jumpToTask(hit.taskKey);
}
