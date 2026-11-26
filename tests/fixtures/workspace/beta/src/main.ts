// Beta scope entry point and its local helper.
// This `shared` has the same name as alpha's but different behavior, so a scope
// leak is observable in both the manifest and the invoke result.
export function betaEntry(x: number): number {
  // 1. Call the local helper so the scanner records a real beta-internal edge.
  return shared(x) - 1;
}

// Beta's `shared`: multiplies by ten, then the entry subtracts one.
export function shared(x: number): number {
  return x * 10;
}
