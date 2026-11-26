import type { FnRecord } from '../lib/types';

/** Placeholder group for files that sit at the scan root. */
export const ROOT_GROUP = '(root)';

export interface ModuleFile {
  path: string;
  label: string;
  functionCount: number;
  functions: FnRecord[];
}

export interface ModuleGroup {
  key: string;
  label: string;
  functionCount: number;
  files: ModuleFile[];
}

/** Manifest paths may use either separator; the tree renders one shape. */
export function normalizePath(filePath: string): string {
  return String(filePath || '')
    .replace(/\\/g, '/')
    .replace(/^\.?\//, '')
    .replace(/\/+/g, '/')
    .replace(/\/$/, '');
}

/** Last segment of a real path, or the whole value when it has no directory. */
export function basename(filePath: string): string {
  const parts = normalizePath(filePath).split('/').filter(Boolean);
  return parts[parts.length - 1] || normalizePath(filePath);
}

/**
 * Directory group for a file. Depth is a caller choice so a deep repository can
 * group by `src/lib` instead of collapsing everything into `src`.
 */
export function groupKey(filePath: string, depth = 1): string {
  const parts = normalizePath(filePath).split('/').filter(Boolean);
  if (parts.length <= depth) return ROOT_GROUP;
  return parts.slice(0, depth).join('/');
}

/** File label relative to its group, so nested rows stay readable. */
export function fileLabel(filePath: string, group: string): string {
  const path = normalizePath(filePath);
  if (group === ROOT_GROUP) return path;
  if (path.startsWith(`${group}/`)) return path.slice(group.length + 1);
  return basename(path);
}

/**
 * Group real manifest functions by directory without a product-specific
 * allowlist. Groups, files and functions are sorted deterministically.
 */
export function buildModuleTree(functions: FnRecord[], depth = 1): ModuleGroup[] {
  const groups = new Map<string, Map<string, FnRecord[]>>();

  for (const fn of functions || []) {
    const path = normalizePath(fn.filePath) || '(unknown)';
    const key = groupKey(path, depth);
    if (!groups.has(key)) groups.set(key, new Map());
    const files = groups.get(key)!;
    if (!files.has(path)) files.set(path, []);
    files.get(path)!.push(fn);
  }

  return [...groups.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([key, files]) => {
      const fileEntries = [...files.entries()]
        .sort(([a], [b]) => a.localeCompare(b))
        .map(([path, fns]) => ({
          path,
          label: fileLabel(path, key),
          functionCount: fns.length,
          functions: [...fns].sort(
            (a, b) => (a.line || 0) - (b.line || 0) || String(a.name).localeCompare(String(b.name))
          ),
        }));

      return {
        key,
        label: key,
        functionCount: fileEntries.reduce((count, file) => count + file.functionCount, 0),
        files: fileEntries,
      };
    });
}
