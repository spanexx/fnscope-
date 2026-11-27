import React from 'react';
import {
  AlertTriangle,
  ArrowDownLeft,
  ArrowRightLeft,
  ArrowUp,
  ArrowUpRight,
  ChevronDown,
  CircleSlash,
  CornerDownRight,
} from 'lucide-react';
import Badge from '../components/Badge';
import type { CallerRef, FnRecord } from '../lib/types';
import { callerTarget, paramLabel } from './helpers';

/** Small inline empty note; it states the absence instead of inventing sample data. */
export function InlineEmpty({ title, detail }: { title: string; detail: string }) {
  return (
    <div className="flex items-start gap-2 rounded-lg border border-dashed border-line-strong bg-surface px-3 py-2.5">
      <CircleSlash className="mt-0.5 h-3.5 w-3.5 shrink-0 text-faint" aria-hidden="true" />
      <div>
        <p className="text-[13px] text-muted">{title}</p>
        <p className="mt-0.5 text-[12px] leading-4 text-faint">{detail}</p>
      </div>
    </div>
  );
}

/** One real caller; Jump is offered only when the target exists in this scope. */
export function CallerCard({
  caller,
  index,
  onCaller,
}: {
  caller: CallerRef;
  index: Map<string, FnRecord>;
  onCaller: (fnId: string) => void;
}) {
  const target = callerTarget(caller, index);
  return (
    <div className="flex items-start gap-2.5 rounded-lg border border-line bg-surface p-2.5">
      <div className="mt-0.5 grid h-6 w-6 shrink-0 place-items-center rounded-md border border-info/20 bg-info/10">
        <ArrowUpRight className="h-3 w-3 text-info" aria-hidden="true" />
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5">
          <span
            className="max-w-full truncate font-mono text-[13.5px] text-ink"
            title={`${caller.name}()`}
          >
            {caller.name}()
          </span>
          <span className="truncate font-mono text-[11.5px] text-muted">{caller.module}</span>
        </div>
        {caller.callSite ? (
          <pre className="mt-1 overflow-hidden text-ellipsis whitespace-nowrap font-mono text-[12px] text-muted">
            {caller.callSite}
          </pre>
        ) : null}
      </div>
      {target ? (
        <button
          type="button"
          onClick={() => onCaller(target.id)}
          className="inline-flex shrink-0 items-center gap-1 rounded border border-info/25 bg-info/10 px-1.5 py-1 font-mono text-[11.5px] text-info transition-colors hover:bg-info/20 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/50"
        >
          Jump
          <ArrowUp className="h-2.5 w-2.5" aria-hidden="true" />
        </button>
      ) : (
        <span className="shrink-0 rounded border border-line-strong px-1.5 py-0.5 font-mono text-[11px] text-faint">
          {caller.fnId ? 'outside scope' : 'external'}
        </span>
      )}
    </div>
  );
}

/** Triggered by: real calledBy records, explicit empty state. */
export function TriggeredBySection({
  fn,
  index,
  onCaller,
}: {
  fn: FnRecord;
  index: Map<string, FnRecord>;
  onCaller: (fnId: string) => void;
}) {
  const callers = fn.calledBy || [];
  return (
    <details open className="group rounded-xl border border-line bg-panel">
      <summary className="flex cursor-pointer select-none items-center gap-2 px-4 py-3 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/50">
        <ArrowDownLeft className="h-4 w-4 text-info" aria-hidden="true" />
        <span className="text-[14px] font-medium text-ink">Triggered by</span>
        <span className="font-mono text-[12px] text-muted">{callers.length}</span>
        <ChevronDown
          className="ml-auto h-3.5 w-3.5 text-muted transition-transform group-open:rotate-180"
          aria-hidden="true"
        />
      </summary>
      <div className="space-y-2 px-4 pb-4">
        {callers.length === 0 ? (
          <InlineEmpty title="No callers in this scope" detail="This is where execution can begin." />
        ) : (
          callers.map((caller, i) => (
            <CallerCard
              key={`${caller.fnId || caller.name}-${i}`}
              caller={caller}
              index={index}
              onCaller={onCaller}
            />
          ))
        )}
      </div>
    </details>
  );
}

/** Inputs: real parameters with defaults, required flag and description. */
export function InputsSection({ fn }: { fn: FnRecord }) {
  const params = fn.parameters || [];
  return (
    <section className="rounded-xl border border-line bg-panel p-4">
      <div className="mb-3 flex items-center gap-2">
        <ArrowRightLeft className="h-4 w-4 text-muted" aria-hidden="true" />
        <span className="text-[14px] font-medium text-ink">Inputs</span>
        <span className="font-mono text-[12px] text-muted">{params.length}</span>
      </div>
      {params.length === 0 ? (
        <InlineEmpty
          title="No parameters"
          detail="This function reads only shared state, or nothing at all."
        />
      ) : (
        <div className="grid gap-2 sm:grid-cols-2">
          {params.map((param, i) => {
            const hasDefault = param.defaultValue !== null && param.defaultValue !== undefined;
            return (
              <div key={`${param.name}-${i}`} className="rounded-lg border border-line bg-surface px-3 py-2">
                <div className="flex items-center justify-between gap-2">
                  <span className="truncate font-mono text-[13.5px] text-ink">{paramLabel(param)}</span>
                  <Badge tone="info">{param.type}</Badge>
                </div>
                <div className="mt-1 flex items-center gap-2 text-[11.5px] text-muted">
                  <span className={param.required ? 'text-err/80' : 'text-muted'}>
                    {param.required ? 'required' : 'optional'}
                  </span>
                  {hasDefault ? (
                    <>
                      <span className="text-faint">·</span>
                      <span className="truncate font-mono">default {JSON.stringify(param.defaultValue)}</span>
                    </>
                  ) : null}
                </div>
                {param.description ? (
                  <p className="mt-1 text-[12px] leading-4 text-muted">{param.description}</p>
                ) : null}
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}

/** Returns and declared errors, both straight from the record. */
export function OutputsSection({ fn }: { fn: FnRecord }) {
  const ret = fn.returns || { type: 'unknown', fields: [] };
  const fields = ret.fields || [];
  const errors = fn.errors || [];
  return (
    <div className="grid gap-3 md:grid-cols-2">
      <section className="rounded-xl border border-line bg-panel p-4">
        <div className="mb-2 flex items-center gap-2">
          <CornerDownRight className="h-4 w-4 text-ok" aria-hidden="true" />
          <span className="text-[14px] font-medium text-ink">Returns</span>
        </div>
        <div className="rounded border border-ok/20 bg-ok/[0.06] px-2 py-1.5 font-mono text-[12.5px] text-ok">
          {ret.type || 'unknown'}
        </div>
        {fields.length ? (
          <div className="mt-2 space-y-1">
            {fields.map((field, i) => (
              <div
                key={`${field.name}-${i}`}
                className="flex items-center justify-between gap-2 rounded border border-line bg-surface px-2 py-1"
              >
                <span className="font-mono text-[12px] text-ink">{field.name}</span>
                <span className="font-mono text-[11px] text-info">{field.type}</span>
              </div>
            ))}
          </div>
        ) : null}
      </section>
      <section className="rounded-xl border border-line bg-panel p-4">
        <div className="mb-2 flex items-center gap-2">
          <AlertTriangle className="h-4 w-4 text-err" aria-hidden="true" />
          <span className="text-[14px] font-medium text-ink">Declared errors</span>
        </div>
        {errors.length ? (
          errors.map((error, i) => (
            <div key={`${error.code}-${i}`} className="mt-1 rounded border border-err/20 bg-err/[0.04] px-2 py-1.5">
              <div className="flex items-center gap-2">
                <span className="font-mono text-[12px] text-err">{error.code}</span>
                {error.status ? <Badge tone="err">{error.status}</Badge> : null}
              </div>
              <p className="mt-0.5 text-[11.5px] leading-4 text-muted">{error.when}</p>
            </div>
          ))
        ) : (
          <InlineEmpty
            title="No declared errors"
            detail="The manifest records no typed error paths for this function."
          />
        )}
      </section>
    </div>
  );
}
