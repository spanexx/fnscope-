import type { FnGraph, FnRecord, GraphSummary, ScopeLanguage, ViewId } from './types';

/** Extension map used only to summarize real manifest files. */
const LANGUAGE_BY_EXTENSION: Record<string, string> = {
  ts: 'TypeScript',
  tsx: 'TSX',
  js: 'JavaScript',
  jsx: 'JSX',
  mjs: 'JavaScript',
  cjs: 'JavaScript',
  rs: 'Rust',
  py: 'Python',
  go: 'Go',
  java: 'Java',
  kt: 'Kotlin',
  cs: 'C#',
  rb: 'Ruby',
  php: 'PHP',
  c: 'C',
  h: 'C',
  cpp: 'C++',
  cc: 'C++',
  hpp: 'C++',
  swift: 'Swift',
  sh: 'Shell',
};

/** Infer a display language from a real file path. */
export function languageForPath(filePath: string): string {
  const clean = String(filePath || '').split(/[?#]/)[0];
  const dot = clean.lastIndexOf('.');
  if (dot < 0) return 'Other';
  return LANGUAGE_BY_EXTENSION[clean.slice(dot + 1).toLowerCase()] || 'Other';
}

/** Index a real graph for O(1) lookups without mutating it. */
export function indexFunctions(graph: FnGraph): Map<string, FnRecord> {
  return new Map((graph.functions || []).map((fn) => [fn.id, fn]));
}

/** Resolve the requested default only if it exists in this scope's graph. */
export function resolveDefaultFunctionId(
  graph: FnGraph,
  preferred?: string | null
): string | null {
  const functions = graph.functions || [];
  if (!functions.length) return null;
  if (preferred && functions.some((fn) => fn.id === preferred)) return preferred;
  return functions[0].id;
}

/** Counts and languages are derived from the manifest, never hardcoded. */
export function graphSummary(graph: FnGraph): GraphSummary {
  const files = new Set<string>();
  const languages = new Map<string, number>();

  for (const fn of graph.functions || []) {
    files.add(fn.filePath);
    const language = languageForPath(fn.filePath);
    languages.set(language, (languages.get(language) || 0) + 1);
  }

  const ranked: ScopeLanguage[] = [...languages.entries()]
    .map(([name, count]) => ({ name, count }))
    .sort((a, b) => b.count - a.count || a.name.localeCompare(b.name));

  return { functionCount: (graph.functions || []).length, fileCount: files.size, languages: ranked };
}

/** A function with zero resolved internal callers is an entry-point candidate. */
export function isEntryPointCandidate(fn: FnRecord): boolean {
  return (fn.calledBy || []).every((caller) => caller.fnId === null || caller.fnId === undefined);
}

/**
 * Rank entry points by step count then name. No function-name allowlist is used;
 * callers can cap the list and expose the hidden count separately.
 */
export function entryPointCandidates(graph: FnGraph): FnRecord[] {
  return (graph.functions || [])
    .filter(isEntryPointCandidate)
    .sort((a, b) => b.steps.length - a.steps.length || a.name.localeCompare(b.name));
}

/** First page of entry points (Phase 4 uses 24). */
export function visibleEntryPoints(graph: FnGraph, limit = 24): FnRecord[] {
  return entryPointCandidates(graph).slice(0, limit);
}

/** Number of candidates hidden behind "show all". */
export function hiddenEntryPointCount(graph: FnGraph, limit = 24): number {
  return Math.max(0, entryPointCandidates(graph).length - limit);
}

/** Wall-clock rendering of generatedAt with a readable fallback. */
export function formatGeneratedAt(value: string): string {
  if (!value) return 'unknown';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' });
}

/** Display order for the four canvas views. */
export function viewIndex(view: ViewId): number {
  return ['story', 'map', 'split', 'raw'].indexOf(view) + 1;
}
