/**
 * Keep the tail of a path readable in dense UI rows without hiding the file.
 */
export function shortPath(filePath: string, keep = 2): string {
  const parts = String(filePath || '')
    .replace(/\\/g, '/')
    .split('/')
    .filter(Boolean);
  if (parts.length <= keep) return parts.join('/');
  return parts.slice(-keep).join('/');
}

/**
 * Format a count with the same grouping the explorer uses everywhere.
 */
export function formatCount(value: number): string {
  return new Intl.NumberFormat('en-US').format(Number(value) || 0);
}
