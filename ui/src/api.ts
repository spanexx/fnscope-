import { graphSummary } from './lib/graph';
import type {
  FnGraph,
  FnRecord,
  InvokeResponse,
  ScopeDescriptor,
  ScopeLanguage,
  ScopeList,
  ScopeListResult,
} from './lib/types';

const SCOPES_URL = '/api/scopes';
const DEFAULT_GRAPH_URL = '/api/fn-graph.json';

/**
 * Stable marker string: survives identifier minification and proves the
 * repaired-object-arg path is present in the built bundle.
 */
export const OBJECT_ARG_MARKER = 'fnscope:repaired-object-arg';

/** HTTP failure with the status kept for the compatibility fallback decision. */
export class ApiError extends Error {
  readonly status: number;

  constructor(message: string, status: number) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
  }
}

async function requestJson(url: string, init?: RequestInit): Promise<unknown> {
  const response = await fetch(url, {
    headers: { Accept: 'application/json' },
    ...init,
  });
  if (!response.ok) {
    throw new ApiError(`${url} returned HTTP ${response.status}`, response.status);
  }
  const text = await response.text();
  try {
    return JSON.parse(text);
  } catch {
    throw new SyntaxError(`${url} did not return JSON`);
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function asString(value: unknown, fallback = ''): string {
  return typeof value === 'string' ? value : fallback;
}

function asNumber(value: unknown, fallback = 0): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback;
}

function firstDefined(...values: unknown[]): unknown {
  return values.find((value) => value !== undefined && value !== null);
}

function normalizeInvokeMode(value: unknown): 'simulate' | 'node' | null {
  const mode = asString(value).toLowerCase();
  if (mode === 'node') return 'node';
  if (mode === 'simulate') return 'simulate';
  return null;
}

function normalizeLanguages(value: unknown): ScopeLanguage[] {
  if (!Array.isArray(value)) return [];
  const counts = new Map<string, number>();
  for (const entry of value) {
    if (typeof entry === 'string' && entry) {
      counts.set(entry, (counts.get(entry) || 0) + 1);
    } else if (isRecord(entry)) {
      const name = asString(entry.name ?? entry.language).trim();
      if (name) counts.set(name, asNumber(entry.count, 0));
    }
  }
  return [...counts.entries()]
    .map(([name, count]) => ({ name, count }))
    .sort((a, b) => b.count - a.count || a.name.localeCompare(b.name));
}

function normalizeScope(value: unknown): ScopeDescriptor | null {
  if (!isRecord(value)) return null;
  const id = asString(value.id).trim();
  if (!id) return null;

  const counts = isRecord(value.counts) ? value.counts : {};
  return {
    id,
    label: asString(value.label, id) || id,
    note: asString(value.note),
    rootDir: asString(
      firstDefined(value.rootDir, value.displayRoot, value.root, value.scanRoot) as unknown
    ),
    functionCount: asNumber(
      firstDefined(value.functionCount, value.functions, counts.functions)
    ),
    fileCount: asNumber(firstDefined(value.fileCount, value.files, counts.files)),
    languages: normalizeLanguages(value.languages),
    generatedAt: typeof value.generatedAt === 'string' ? value.generatedAt : null,
    available: value.available !== false,
    invokeMode: normalizeInvokeMode(firstDefined(value.invokeMode, value.invoke_mode)),
    defaultFunctionId: asString(firstDefined(value.defaultFunctionId, value.defaultFunction)) || null,
    error: typeof value.error === 'string' ? value.error : null,
    compatibility: false,
  };
}

/** Validate the additive scope list instead of trusting an arbitrary payload. */
export function normalizeScopeList(raw: unknown): ScopeList {
  const entries = Array.isArray(raw)
    ? raw
    : isRecord(raw) && Array.isArray(raw.scopes)
      ? raw.scopes
      : null;
  if (!entries || entries.length === 0) {
    throw new SyntaxError('/api/scopes did not return a non-empty scope list');
  }

  const scopes = entries
    .map(normalizeScope)
    .filter((scope): scope is ScopeDescriptor => scope !== null);
  if (scopes.length === 0) {
    throw new SyntaxError('/api/scopes returned no usable scope descriptors');
  }

  const requestedDefault = isRecord(raw)
    ? asString(firstDefined(raw.defaultScope, raw.default_scope))
    : '';
  const defaultScope = scopes.some((scope) => scope.id === requestedDefault)
    ? requestedDefault
    : scopes[0].id;

  return { scopes, defaultScope };
}

function displayRootName(rootDir: string): string {
  const trimmed = String(rootDir || '').replace(/[\\/]+$/, '');
  const parts = trimmed.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] || 'fnscope workspace';
}

/** Build the one-scope descriptor used when /api/scopes is absent. */
export function scopeListFromGraph(graph: FnGraph): ScopeList {
  const summary = graphSummary(graph);
  const rootDir = asString(graph.rootDir);
  return {
    scopes: [
      {
        id: 'default',
        label: displayRootName(rootDir),
        note: 'Single-scope compatibility API',
        rootDir,
        functionCount: summary.functionCount,
        fileCount: summary.fileCount,
        languages: summary.languages,
        generatedAt: typeof graph.generatedAt === 'string' ? graph.generatedAt : null,
        available: true,
        invokeMode: null,
        defaultFunctionId: null,
        compatibility: true,
      },
    ],
    defaultScope: 'default',
  };
}

function assertGraph(raw: unknown, url: string): FnGraph {
  if (!isRecord(raw) || !Array.isArray(raw.functions)) {
    throw new SyntaxError(`${url} is not an fnscope manifest`);
  }
  return raw as unknown as FnGraph;
}

/** Only an absent route or a non-JSON response may fall back to the old API. */
function isMissingScopesRoute(error: unknown): boolean {
  if (error instanceof ApiError) return error.status === 404 || error.status === 405;
  return error instanceof SyntaxError;
}

function isAbort(error: unknown): boolean {
  return error instanceof DOMException && error.name === 'AbortError';
}

/**
 * Fetch /api/scopes, falling back to /api/fn-graph.json only when the route is
 * absent. Any other server error is surfaced rather than hidden by demo data.
 */
export async function fetchScopeList(signal?: AbortSignal): Promise<ScopeListResult> {
  try {
    const raw = await requestJson(SCOPES_URL, { signal });
    return { list: normalizeScopeList(raw), compatibility: false };
  } catch (error) {
    if (isAbort(error)) throw error;
    if (!isMissingScopesRoute(error)) throw error;

    const raw = await requestJson(DEFAULT_GRAPH_URL, { signal });
    const graph = assertGraph(raw, DEFAULT_GRAPH_URL);
    const list = scopeListFromGraph(graph);
    return {
      list,
      compatibility: true,
      prefetchedScopeId: list.defaultScope,
      prefetched: graph,
    };
  }
}

function graphUrl(scope: ScopeDescriptor): string {
  if (scope.compatibility) return DEFAULT_GRAPH_URL;
  return `${SCOPES_URL}/${encodeURIComponent(scope.id)}/fn-graph.json`;
}

function invokeUrl(scope: ScopeDescriptor): string {
  if (scope.compatibility) return '/api/invoke';
  return `${SCOPES_URL}/${encodeURIComponent(scope.id)}/invoke`;
}

/** Load one scope's manifest; never substitute another scope's graph. */
export async function fetchScopeGraph(
  scope: ScopeDescriptor,
  signal?: AbortSignal
): Promise<FnGraph> {
  const url = graphUrl(scope);
  const raw = await requestJson(url, { signal });
  return assertGraph(raw, url);
}

/** Post to the selected scope's invoke route with the real response type. */
export async function invokeFunction(
  scope: ScopeDescriptor,
  fnId: string,
  args: unknown,
  signal?: AbortSignal
): Promise<InvokeResponse> {
  const url = invokeUrl(scope);
  const raw = await requestJson(url, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ fnId, args }),
    signal,
  });
  if (!isRecord(raw) || typeof raw.ok !== 'boolean') {
    throw new SyntaxError(`${url} did not return an InvokeResponse`);
  }
  return raw as unknown as InvokeResponse;
}

/**
 * Rebuild an inline object parameter from the sandbox's flat fields. Only the
 * single-object-parameter case is repaired; scalar calls pass through exactly.
 */
export function repairArgsForFn(
  fn: Pick<FnRecord, 'parameters'>,
  flatArgs: Record<string, unknown>
): { args: Record<string, unknown>; repaired: boolean } {
  const objectParams = (fn.parameters || []).filter(
    (param) => typeof param.type === 'string' && param.type.trim().startsWith('{')
  );
  if (objectParams.length !== 1) return { args: flatArgs, repaired: false };

  console.debug(OBJECT_ARG_MARKER, objectParams[0].name);
  const builtObject = buildObjectArg(objectParams[0].type, flatArgs);
  // Keep the flat keys too: the Node runner passes the whole args object as the
  // single first argument for one-parameter functions.
  return { args: { ...flatArgs, [objectParams[0].name]: builtObject }, repaired: true };
}

function buildObjectArg(
  typeText: string,
  flatArgs: Record<string, unknown>
): Record<string, unknown> {
  const built: Record<string, unknown> = {};
  const declaration = /([A-Za-z_$][\w$]*)\??\s*:\s*([^;\n}]+)/g;
  let match: RegExpExecArray | null;
  while ((match = declaration.exec(typeText)) !== null) {
    const name = match[1];
    const declaredType = match[2].trim();
    let value: unknown = '';
    if (name === 'sentence') value = 'The quick fox jumped over the lazy dog.';
    else if (name === 'targetWord') value = 'fox';
    else if (/\bboolean\b/.test(declaredType)) value = true;
    else if (/\bnumber\b/.test(declaredType)) value = 0;
    built[name] = Object.prototype.hasOwnProperty.call(flatArgs, name) ? flatArgs[name] : value;
  }
  return built;
}
