/**
 * Scope selection persistence. A stored id is only trusted after validation
 * against the server's current scope list.
 */
export const SCOPE_STORAGE_KEY = 'fnscope-workspace-scope';

export function readStoredScopeId(): string | null {
  try {
    return window.localStorage.getItem(SCOPE_STORAGE_KEY);
  } catch {
    return null;
  }
}

export function rememberScopeId(scopeId: string): void {
  try {
    window.localStorage.setItem(SCOPE_STORAGE_KEY, scopeId);
  } catch {
    // Storage can be blocked; the in-memory selection still works.
  }
}

/**
 * Precedence: valid stored scope > server default > first available scope.
 * Unknown/unavailable ids fall back instead of rendering an empty shell.
 */
export function pickInitialScopeId(
  scopes: { id: string; available: boolean }[],
  defaultScope: string
): string | null {
  if (scopes.length === 0) return null;
  const stored = readStoredScopeId();
  const storedScope = stored ? scopes.find((scope) => scope.id === stored) : null;
  if (storedScope && storedScope.available) return storedScope.id;

  const serverDefault = scopes.find((scope) => scope.id === defaultScope);
  if (serverDefault && serverDefault.available) return serverDefault.id;

  const firstAvailable = scopes.find((scope) => scope.available);
  return (firstAvailable || scopes[0]).id;
}
