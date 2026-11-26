import type { FnRecord } from '../lib/types';

/** One step in the call path; labels are display text, ids stay canonical. */
export interface TrailCrumb {
  id: string;
  label: string;
}

/** Build a crumb from a real manifest record, including the call suffix. */
export function crumbFor(fn: FnRecord): TrailCrumb {
  return { id: fn.id, label: `${fn.name}()` };
}

/** Selecting from the explorer starts a fresh trail. */
export function startTrail(crumb: TrailCrumb): TrailCrumb[] {
  return [crumb];
}

/**
 * Walking to a caller records the new function at the front so Back retraces
 * the same path. Revisits are kept; the trail is a history, not a set.
 */
export function prependCrumb(trail: TrailCrumb[], crumb: TrailCrumb): TrailCrumb[] {
  return [crumb, ...trail];
}

/** Walking to a callee appends; the current function stays in the middle. */
export function appendCrumb(trail: TrailCrumb[], crumb: TrailCrumb): TrailCrumb[] {
  return [...trail, crumb];
}

/**
 * Clicking an earlier crumb shortens the trail to that point. Out-of-range
 * indexes are ignored so a stale click cannot empty the trail.
 */
export function truncateTrail(trail: TrailCrumb[], index: number): TrailCrumb[] {
  if (!Number.isInteger(index) || index < 0 || index >= trail.length) return trail;
  return trail.slice(0, index + 1);
}

/** Back drops the current crumb; at the root the trail is unchanged. */
export function backTrail(trail: TrailCrumb[]): TrailCrumb[] {
  return trail.length > 1 ? trail.slice(0, -1) : trail;
}

/** The crumb the canvas is showing, or null for an empty trail. */
export function currentCrumb(trail: TrailCrumb[]): TrailCrumb | null {
  return trail.length ? trail[trail.length - 1] : null;
}
