// Errors carry a stable machine code so callers can map them to HTTP statuses.
export class FunctionError extends Error {
  // Type-only declarations are erased by Node's TypeScript stripper.
  declare code: string;
  declare details: Record<string, unknown>;

  constructor(code: string, details: Record<string, unknown> = {}) {
    super(`${code}: ${JSON.stringify(details)}`);
    this.name = 'FunctionError';
    this.code = code;
    this.details = details;
  }
}
