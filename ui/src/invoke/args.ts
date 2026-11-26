/**
 * Argument helpers for the invoke drawer. Templates come from the function's
 * real parameters and declared defaults, never from manifest step guesses.
 */
import type { FnRecord, Parameter } from '../lib/types';

/** A JSON parse failure with an optional character offset for the caret. */
export interface ArgsError {
  message: string;
  position: number | null;
}

export type ParsedArgs =
  | { ok: true; value: Record<string, unknown> }
  | { ok: false; error: ArgsError };

export interface ArgsTemplate {
  value: Record<string, unknown>;
  text: string;
  /** Parameter names whose value is a type-derived sample, not a default. */
  sampled: string[];
}

const OBJECT_TYPE = /^\s*\{/;
const NOT_OBJECT_ERROR = 'Arguments must be a JSON object literal.';

/** Parse `{ a: string; b?: number }` fields into a JSON-safe sample object. */
function objectSample(typeText: string): Record<string, unknown> {
  const fields: Record<string, unknown> = {};
  const declaration = /([A-Za-z_$][\w$]*)\??\s*:\s*([^;,\n}]+)/g;
  let match: RegExpExecArray | null;
  while ((match = declaration.exec(typeText)) !== null) {
    fields[match[1]] = sampleForType(match[2].trim());
  }
  return fields;
}

/**
 * Build a JSON value from a declared TypeScript type. Unknown types become null
 * rather than a made-up value, so the editor never pretends to know the shape.
 */
export function sampleForType(type: string): unknown {
  const text = String(type || '').trim();
  if (!text) return null;
  if (OBJECT_TYPE.test(text)) return objectSample(text);
  if (/^(?:[A-Za-z_$][\w$]*\s*\|\s*)*(?:null|undefined)\b/.test(text)) return null;
  if (/\b(?:null|undefined|Option|Optional|Maybe)\b/.test(text)) return null;
  if (/^(?:true|false)$/i.test(text)) return text.toLowerCase() === 'true';
  if (/^(?:'[^']*'|"[^"]*")$/.test(text)) return text.slice(1, -1);
  if (/\b(?:boolean|bool)\b/i.test(text)) return false;
  if (/\b(?:string|str|char)\b/i.test(text)) return '';
  if (
    /\b(?:number|int|integer|float|double|usize|isize|u8|u16|u32|u64|i8|i16|i32|i64|f32|f64)\b/i.test(
      text
    )
  ) {
    return 0;
  }
  if (/\[\]|\b(?:Array|Vec|Set|BTreeSet|HashSet|slice)\b/.test(text)) return [];
  if (/\b(?:Map|Record|object)\b/.test(text)) return {};
  return null;
}

/** Function parameter names are already identifiers; keep a readable fallback. */
export function paramLabel(param: Parameter): string {
  const name = String(param.name || '').trim();
  return name || 'arg';
}

/** Render the manifest signature the drawer shows above the function name. */
export function formatSignature(
  fn: Pick<FnRecord, 'name' | 'parameters' | 'returns'>
): string {
  const params = (fn.parameters || [])
    .map((param) => `${paramLabel(param)}: ${param.type || 'unknown'}`)
    .join(', ');
  const returns = fn.returns && fn.returns.type ? `: ${fn.returns.type}` : '';
  return `${fn.name}(${params})${returns}`;
}

/**
 * Seed the editor with declared defaults; parameters without one get a sample
 * so the JSON is always shaped like the real call.
 */
export function buildArgsTemplate(fn: Pick<FnRecord, 'parameters'>): ArgsTemplate {
  const value: Record<string, unknown> = {};
  const sampled: string[] = [];
  for (const param of fn.parameters || []) {
    const name = paramLabel(param);
    if (param.defaultValue !== undefined) {
      value[name] = param.defaultValue;
    } else {
      value[name] = sampleForType(param.type);
      sampled.push(name);
    }
  }
  return { value, text: JSON.stringify(value, null, 2), sampled };
}

/** One-line parameter legend under the editor, e.g. `sentence: string`. */
export function parameterSummary(parameters: Parameter[]): string {
  if (!parameters || parameters.length === 0) {
    return 'No declared parameters; blank arguments send {}.';
  }
  return parameters
    .map((param) => {
      const label = `${paramLabel(param)}: ${param.type || 'unknown'}`;
      if (param.defaultValue !== undefined) {
        return `${label} = ${JSON.stringify(param.defaultValue)}`;
      }
      return param.required ? label : `${label}?`;
    })
    .join(' \u00b7 ');
}

function jsonErrorPosition(message: string): number | null {
  const match =
    message.match(/position (\d+)/i) ||
    message.match(/at (\d+)/i) ||
    message.match(/column (\d+)/i);
  if (!match) return null;
  const value = Number(match[1]);
  return Number.isFinite(value) ? value : null;
}

/**
 * Parse editor text into a plain args object. Blank input is a valid call with
 * no arguments; arrays, scalars and null are rejected before the request.
 */
export function parseArgsInput(text: string): ParsedArgs {
  const raw = String(text ?? '').trim();
  if (!raw) return { ok: true, value: {} };

  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    return {
      ok: false,
      error: { message: `Invalid JSON: ${message}`, position: jsonErrorPosition(message) },
    };
  }

  if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
    return { ok: false, error: { message: NOT_OBJECT_ERROR, position: null } };
  }
  return { ok: true, value: parsed as Record<string, unknown> };
}
