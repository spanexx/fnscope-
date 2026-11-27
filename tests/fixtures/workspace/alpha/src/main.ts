// Alpha scope entry point and its local helper.
// `shared` is deliberately named the same as beta's helper: identical names in two
// scopes must still resolve to two different functions because each scope is scanned
// and invoked independently.
export function alphaEntry(x: number): number {
  // 1. Call the local helper so the scanner records a real alpha-internal edge.
  const doubled = shared(x);
  return doubled + 1;
}

// Alpha's `shared`: doubles the input, then the entry adds one.
export function shared(x: number): number {
  return x * 2;
}
