import type { CallerRef, FnRecord, Parameter, Step, StepKind } from '../lib/types';

/** Badge tones mirror the shared Badge primitive without importing JSX into a helper. */
export type Tone = 'neutral' | 'accent' | 'info' | 'ok' | 'warn' | 'err';

/** Manifest parameter names sometimes carry a parenthesised form; show the inner name. */
export function paramLabel(param: Parameter): string {
  const match = String(param.name || '').match(/\(([^)]+)\)/);
  return match ? match[1] : param.name;
}

/** Build the signature only from real manifest fields, never a curated string. */
export function signature(fn: FnRecord): string {
  const params = (fn.parameters || []).map((p) => `${paramLabel(p)}: ${p.type}`).join(', ');
  const ret = fn.returns && fn.returns.type ? `: ${fn.returns.type}` : '';
  return `${fn.name}(${params})${ret}`;
}

/** Map a manifest step kind to a chip; unknown kinds stay neutral. */
export function kindMeta(kind: StepKind): { label: string; tone: Tone } {
  switch (kind) {
    case 'call':
      return { label: 'call', tone: 'info' };
    case 'branch':
      return { label: 'branch', tone: 'warn' };
    case 'return':
      return { label: 'return', tone: 'ok' };
    case 'transform':
      return { label: 'transform', tone: 'neutral' };
    default:
      return { label: String(kind || 'step'), tone: 'neutral' };
  }
}

/** Resolve a caller target only if it exists in this scope's real index. */
export function callerTarget(caller: CallerRef, index: Map<string, FnRecord>): FnRecord | null {
  if (!caller.fnId) return null;
  return index.get(caller.fnId) || null;
}

/** Resolve the record a real step calls, if that callee lives in this scope. */
export function stepCallee(step: Step, index: Map<string, FnRecord>): FnRecord | null {
  if (!step.callsFnId) return null;
  return index.get(step.callsFnId) || null;
}

/** Shorten a module path generically for a dense badge, without inventing a name. */
export function moduleLabel(module: string): string {
  const raw = String(module || '');
  if (!raw) return 'module';
  const parts = raw.split('/').filter(Boolean);
  return parts.slice(-2).join('/') || raw;
}
