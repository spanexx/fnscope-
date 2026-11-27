import React from 'react';
import { ArrowDownLeft, ArrowRight, ArrowUpRight, Crosshair } from 'lucide-react';
import Badge from '../components/Badge';
import { indexFunctions, isEntryPointCandidate, languageForPath } from '../lib/graph';
import type { CallerRef, FnRecord, Step } from '../lib/types';
import { callerTarget, moduleLabel, signature } from './helpers';
import type { CanvasViewProps } from './props';

/** 1-Hop canvas: real callers on the left, the record in the centre, real callees right. */
export default function MapView({ graph, fn, onCaller, onCallee, trail }: CanvasViewProps) {
  if (!fn) return null;
  const index = indexFunctions(graph);
  const language = languageForPath(fn.filePath);
  const callers = fn.calledBy || [];

  // One callee per distinct real callsFnId, preserving first appearance order.
  const calleeMap = new Map<string, Step>();
  for (const step of fn.steps || []) {
    if (step.callsFnId && !calleeMap.has(step.callsFnId)) calleeMap.set(step.callsFnId, step);
  }
  const callees = [...calleeMap.entries()];

  return (
    <div className="mx-auto w-full max-w-[1120px] px-5 py-5 sm:px-7">
      <div className="flex flex-wrap items-end justify-between gap-2">
        <div>
          <div className="flex items-center gap-2">
            <span className="text-[12px] font-semibold uppercase tracking-[0.14em] text-muted">
              1-Hop call neighborhood
            </span>
            <Badge tone="neutral">{language}</Badge>
          </div>
          <div className="mt-1 font-mono text-[18px] text-ink">{fn.name}()</div>
          {trail && trail.length ? (
            <p
              className="mt-1 truncate font-mono text-[11.5px] text-faint"
              title={trail.map((crumb) => crumb.label).join(' \u203a ')}
            >
              Path: {trail.map((crumb) => crumb.label).join(' \u203a ')}
            </p>
          ) : null}
        </div>
        <p className="max-w-[420px] text-[12.5px] leading-4 text-muted">
          Click a caller to prepend it to the call path, or a callee to append it. The current
          function stays in the centre.
        </p>
      </div>

      <div className="mt-4 grid items-start gap-3 lg:grid-cols-[1fr_1.25fr_1fr]">
        <div>
          <div className="mb-2 flex items-center gap-1.5 text-[12px] uppercase tracking-[0.12em] text-muted">
            <ArrowDownLeft className="h-3 w-3 text-info" aria-hidden="true" />
            Callers <span className="font-mono">{callers.length}</span>
          </div>
          <div className="space-y-2">
            {callers.length ? (
              callers.map((caller, i) => (
                <MapCallerCard
                  key={`${caller.fnId || caller.name}-${i}`}
                  caller={caller}
                  index={index}
                  onCaller={onCaller}
                />
              ))
            ) : (
              <MapEmpty label="No callers" />
            )}
          </div>
        </div>

        <div>
          <div className="mb-2 flex items-center gap-1.5 text-[12px] uppercase tracking-[0.12em] text-accent">
            <Crosshair className="h-3 w-3" aria-hidden="true" />
            Current function
          </div>
          <div className="rounded-xl border border-accent/40 bg-accent/[0.06] p-4">
            <div className="flex flex-wrap gap-1">
              <Badge tone="neutral" title={fn.module}>
                {moduleLabel(fn.module)}
              </Badge>
              <Badge tone={fn.purity === 'pure' ? 'ok' : 'warn'}>
                {fn.purity === 'pure' ? 'pure' : 'side-effect'}
              </Badge>
              {isEntryPointCandidate(fn) ? <Badge tone="accent">entry point</Badge> : null}
            </div>
            <div className="mt-2 break-words font-mono text-[16px] leading-6 text-ink">
              {signature(fn)}
            </div>
            <p className="mt-2 text-[13px] leading-5 text-muted">
              {fn.description || 'No description in the manifest.'}
            </p>
            <div className="mt-3 font-mono text-[11.5px] text-muted">
              {fn.filePath}:{fn.line}
            </div>
          </div>
          <div className="mt-2 rounded-lg border border-dashed border-line-strong px-3 py-2 text-[12px] leading-4 text-muted">
            <span className="text-muted">Story tip:</span> the breadcrumb records the path you take;
            use <span className="font-mono text-muted">&larr;</span> to step back.
          </div>
        </div>

        <div>
          <div className="mb-2 flex items-center gap-1.5 text-[12px] uppercase tracking-[0.12em] text-muted">
            <ArrowUpRight className="h-3 w-3 text-ok" aria-hidden="true" />
            Callees <span className="font-mono">{callees.length}</span>
          </div>
          <div className="space-y-2">
            {callees.length ? (
              callees.map(([id, step]) => (
                <MapCalleeCard
                  key={id}
                  id={id}
                  step={step}
                  index={index}
                  onCallee={onCallee}
                />
              ))
            ) : (
              <MapEmpty label="No resolved callees" />
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

/** A real caller card; Jump is offered only when the target resolves in-scope. */
function MapCallerCard({
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
    <div className="rounded-lg border border-line bg-panel p-2.5">
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <div className="truncate font-mono text-[13px] text-ink">{caller.name}()</div>
          <div className="mt-0.5 truncate font-mono text-[11px] text-muted">{caller.module}</div>
        </div>
        {target ? (
          <button
            type="button"
            onClick={() => onCaller(target.id)}
            className="inline-flex shrink-0 items-center gap-1 rounded border border-info/25 bg-info/10 px-1.5 py-0.5 font-mono text-[11px] text-info transition-colors hover:bg-info/20 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/50"
          >
            Jump
            <ArrowDownLeft className="h-2.5 w-2.5" aria-hidden="true" />
          </button>
        ) : (
          <span className="shrink-0 rounded border border-line-strong px-1.5 py-0.5 font-mono text-[10px] text-faint">
            {caller.fnId ? 'outside scope' : 'external'}
          </span>
        )}
      </div>
    </div>
  );
}

/** A real callee card; unresolved callees stay visible but marked out of scope. */
function MapCalleeCard({
  id,
  step,
  index,
  onCallee,
}: {
  id: string;
  step: Step;
  index: Map<string, FnRecord>;
  onCallee: (fnId: string) => void;
}) {
  const target = index.get(id);
  if (!target) {
    return (
      <div className="rounded-lg border border-dashed border-line-strong bg-surface p-2.5 opacity-70">
        <div className="truncate font-mono text-[13px] text-muted">{id}</div>
        <div className="mt-0.5 text-[11px] text-faint">outside this scope</div>
      </div>
    );
  }
  return (
    <button
      type="button"
      onClick={() => onCallee(target.id)}
      className="w-full rounded-lg border border-line bg-panel p-2.5 text-left transition-colors hover:border-ok/30 hover:bg-ok/[0.04] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/50"
    >
      <div className="flex items-center justify-between gap-2">
        <span className="truncate font-mono text-[13px] text-ink">{target.name}()</span>
        <ArrowRight className="h-3 w-3 shrink-0 text-ok/70" aria-hidden="true" />
      </div>
      <div className="mt-0.5 truncate text-[11px] text-muted">{step.title}</div>
    </button>
  );
}

/** Honest dashed placeholder for an empty side of the neighbourhood. */
function MapEmpty({ label }: { label: string }) {
  return (
    <div className="rounded-lg border border-dashed border-line-strong px-3 py-6 text-center text-[12px] text-faint">
      {label}
    </div>
  );
}
