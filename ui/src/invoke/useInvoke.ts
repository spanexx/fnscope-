/**
 * One drawer's invoke lifecycle. A token plus AbortController keeps a stale
 * response from overwriting the trace of the function now on screen.
 */
import { useCallback, useEffect, useRef, useState } from 'react';
import { invokeFunction, repairArgsForFn } from '../api';
import type { FnRecord, InvokeResponse, ScopeDescriptor } from '../lib/types';

export type InvokeOutcome =
  | { status: 'idle' }
  | { status: 'running' }
  | { status: 'done'; response: InvokeResponse; repaired: boolean }
  | { status: 'failed'; message: string };

export interface InvokeController {
  outcome: InvokeOutcome;
  running: boolean;
  run: (parsed: Record<string, unknown>) => Promise<void>;
  reset: () => void;
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}

function isAbortError(error: unknown): boolean {
  return error instanceof DOMException && error.name === 'AbortError';
}

export function useInvoke(scope: ScopeDescriptor, fn: FnRecord): InvokeController {
  const [outcome, setOutcome] = useState<InvokeOutcome>({ status: 'idle' });
  const controllerRef = useRef<AbortController | null>(null);
  const tokenRef = useRef(0);
  // Scope and function are separate manifest facts; either change invalidates a run.
  const identity = `${scope?.id ?? ''}\u0000${fn?.id ?? ''}`;

  const cancel = useCallback(() => {
    controllerRef.current?.abort();
    controllerRef.current = null;
    tokenRef.current += 1;
  }, []);

  useEffect(() => {
    cancel();
    setOutcome({ status: 'idle' });
  }, [identity, cancel]);

  // Closing the drawer unmounts the hook, so drop any in-flight request too.
  useEffect(() => cancel, [cancel]);

  const run = useCallback(
    async (parsed: Record<string, unknown>) => {
      const controller = new AbortController();
      cancel();
      controllerRef.current = controller;
      const token = ++tokenRef.current;
      setOutcome({ status: 'running' });

      const { args, repaired } = repairArgsForFn(fn, parsed);
      try {
        const response = await invokeFunction(scope, fn.id, args, controller.signal);
        if (token !== tokenRef.current) return;
        setOutcome({ status: 'done', response, repaired });
      } catch (error) {
        if (token !== tokenRef.current || controller.signal.aborted || isAbortError(error)) {
          return;
        }
        setOutcome({ status: 'failed', message: errorMessage(error) });
      } finally {
        if (token === tokenRef.current) controllerRef.current = null;
      }
    },
    [cancel, scope, fn]
  );

  const reset = useCallback(() => {
    cancel();
    setOutcome({ status: 'idle' });
  }, [cancel]);

  return { outcome, running: outcome.status === 'running', run, reset };
}
