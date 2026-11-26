import { entryPointCandidates, hiddenEntryPointCount, visibleEntryPoints } from '../lib/graph';
import type { FnGraph, FnRecord } from '../lib/types';

/** First page of entry candidates shown before "show all". */
export const ENTRY_POINT_LIMIT = 24;

/** Search results are capped so a broad filter cannot flood the sidebar. */
export const SEARCH_LIMIT = 60;

export interface EntryPointPage {
  items: FnRecord[];
  total: number;
  hidden: number;
}

/** Entry candidates honour the 24 cap unless the user asked for everything. */
export function entryPointPage(
  graph: FnGraph,
  showAll: boolean,
  limit = ENTRY_POINT_LIMIT
): EntryPointPage {
  return {
    items: showAll ? entryPointCandidates(graph) : visibleEntryPoints(graph, limit),
    total: entryPointCandidates(graph).length,
    hidden: hiddenEntryPointCount(graph, limit),
  };
}

/** Header count that matches the list's ranking source. */
export function entryPointCount(graph: FnGraph | null): number {
  return graph ? entryPointCandidates(graph).length : 0;
}

export interface FunctionSearchResult {
  matches: FnRecord[];
  total: number;
}

/**
 * Case-insensitive match over name, id, module and file path. Exact names and
 * ids rank first, then prefixes, then the largest step counts; the result is
 * capped but the caller still sees how many functions matched in total.
 */
export function searchFunctions(
  graph: FnGraph | null,
  query: string,
  limit = SEARCH_LIMIT
): FunctionSearchResult {
  if (!graph) return { matches: [], total: 0 };
  const needle = query.trim().toLowerCase();
  if (!needle) return { matches: [], total: 0 };

  const matches = (graph.functions || []).filter((fn) =>
    [fn.name, fn.id, fn.module, fn.filePath].some((value) =>
      String(value ?? '').toLowerCase().includes(needle)
    )
  );

  matches.sort((a, b) => {
    const aRank = matchRank(a, needle);
    const bRank = matchRank(b, needle);
    return (
      aRank - bRank ||
      (b.steps || []).length - (a.steps || []).length ||
      String(a.name).localeCompare(String(b.name))
    );
  });

  return { matches: matches.slice(0, Math.max(0, limit)), total: matches.length };
}

/** 0 = exact field, 1 = field prefix, 2 = substring elsewhere. */
function matchRank(fn: FnRecord, needle: string): number {
  const fields = [fn.name, fn.id, fn.module, fn.filePath].map((value) =>
    String(value ?? '').toLowerCase()
  );
  if (fields.some((value) => value === needle)) return 0;
  if (fields.some((value) => value.startsWith(needle))) return 1;
  return 2;
}
