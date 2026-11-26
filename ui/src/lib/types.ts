/**
 * Locked manifest and API shapes from docs/contract/rust-api.md, plus the
 * additive scope descriptor that the workspace server lists.
 */

/** Purity is a manifest fact, never a UI guess. */
export type Purity = 'pure' | 'side-effect';

/** Step kinds come straight from the scanner. */
export type StepKind = 'call' | 'branch' | 'transform' | 'return';

/** The four canvas views, in keyboard order 1-4. */
export type ViewId = 'story' | 'map' | 'split' | 'raw';

/** Canvas views in switcher order; index + 1 is the shortcut digit. */
export const VIEW_IDS: ViewId[] = ['story', 'map', 'split', 'raw'];

/** Palette names persisted in localStorage. */
export type ThemeName = 'paper' | 'graphite';

/** Server capability; null means the legacy API did not report one. */
export type InvokeMode = 'simulate' | 'node';

/** Coarse shell state; placeholders render each state honestly. */
export type ShellStatus = 'loading' | 'error' | 'empty' | 'ready';

export interface CallerRef {
  fnId: string | null;
  name: string;
  module: string;
  callSite: string;
}

export interface Parameter {
  name: string;
  type: string;
  required: boolean;
  defaultValue?: unknown;
  description: string;
}

export interface Step {
  id: string;
  kind: StepKind;
  callsFnId: string | null;
  title: string;
  code: string;
  inputs: string[];
  output: string;
  detail: string;
}

export interface ReturnField {
  name: string;
  type: string;
}

export interface ReturnShape {
  type: string;
  fields: ReturnField[];
}

export interface ErrorSpec {
  code: string;
  status: number | null;
  when: string;
}

export interface FnRecord {
  id: string;
  name: string;
  module: string;
  filePath: string;
  line: number;
  astHash: string;
  version: string;
  runtime: string;
  purity: Purity;
  timeoutMs: number;
  permissions: string[];
  description: string;
  tags: string[];
  calledBy: CallerRef[];
  parameters: Parameter[];
  steps: Step[];
  returns: ReturnShape;
  errors: ErrorSpec[];
  sourceCode: string;
}

export interface FnGraph {
  tool: string;
  schemaVersion: string;
  generatedAt: string;
  rootDir: string;
  functions: FnRecord[];
}

export interface TraceEntry {
  status: string;
  output: string;
}

export interface InvokeRequest {
  fnId: string;
  args: unknown;
}

export interface InvokeResponse {
  ok: boolean;
  data?: unknown;
  error?: string | null;
  trace: Record<string, TraceEntry>;
  durationMs: number;
}

export interface ScopeLanguage {
  name: string;
  count: number;
}

/**
 * ScopeDescriptor mirrors the PRD's scope list fields. `rootDir` is always a
 * display root supplied by the server, never a client-authored absolute path.
 */
export interface ScopeDescriptor {
  id: string;
  label: string;
  note: string;
  rootDir: string;
  functionCount: number;
  fileCount: number;
  languages: ScopeLanguage[];
  available: boolean;
  invokeMode: InvokeMode | null;
  defaultFunctionId: string | null;
  error?: string | null;
  /** True for the single-scope compatibility descriptor built from fn-graph.json. */
  compatibility?: boolean;
}

export interface ScopeList {
  scopes: ScopeDescriptor[];
  defaultScope: string;
}

/** What the initial scope-list request learned, including a compatibility graph. */
export interface ScopeListResult {
  list: ScopeList;
  compatibility: boolean;
  prefetchedScopeId?: string;
  prefetched?: FnGraph;
}

export interface GraphSummary {
  functionCount: number;
  fileCount: number;
  languages: ScopeLanguage[];
}
