import React, { useState } from 'react';
import { ArrowRight, ChevronsDown, ChevronsUp, Route } from 'lucide-react';
import Badge from '../components/Badge';
import CodeBlock from '../components/CodeBlock';
import type { FnRecord, Step } from '../lib/types';
import { kindMeta, stepCallee } from './helpers';
import { InlineEmpty } from './Panels';

const INITIAL_STEPS = 6;

/** Tone-to-text map keeps compact step chips legible without the Badge border. */
const TONE_TEXT: Record<string, string> = {
  ok: 'text-ok',
  warn: 'text-warn',
  info: 'text-info',
  accent: 'text-accent',
  err: 'text-err',
  neutral: 'text-muted',
};

interface StepListProps {
  fn: FnRecord;
  index: Map<string, FnRecord>;
  onCallee: (fnId: string) => void;
}

/** Full execution story: six real steps first, expand on demand, real step ids. */
export function StepList({ fn, index, onCallee }: StepListProps) {
  const [limit, setLimit] = useState(INITIAL_STEPS);
  const steps = fn.steps || [];
  const shown = steps.slice(0, limit);
  const canExpand = steps.length > shown.length;
  const canCollapse = limit > INITIAL_STEPS;

  return (
    <section className="rounded-xl border border-line bg-panel">
      <div className="flex items-center gap-2 border-b border-line/80 px-4 py-3">
        <Route className="h-4 w-4 text-accent" aria-hidden="true" />
        <span className="text-[14px] font-medium text-ink">Execution story</span>
        <span className="font-mono text-[12px] text-muted">
          {shown.length}/{steps.length}
        </span>
        <span className="ml-auto hidden text-[12px] text-faint sm:inline">ordered by execution</span>
      </div>
      <div className="p-4">
        {steps.length === 0 ? (
          <InlineEmpty title="No steps extracted" detail="The parser found no body facts for this function." />
        ) : (
          <ol className="relative">
            {shown.map((step, i) => (
              <StepRow
                key={step.id || i}
                step={step}
                position={i}
                index={index}
                onCallee={onCallee}
              />
            ))}
          </ol>
        )}
        {canExpand ? (
          <button
            type="button"
            onClick={() => setLimit((current) => current + INITIAL_STEPS)}
            className="mt-2 flex w-full items-center justify-center gap-1 rounded border border-dashed border-line-strong px-2 py-1.5 text-[12px] text-muted transition-colors hover:border-line-strong hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/50"
          >
            <ChevronsDown className="h-3 w-3" aria-hidden="true" />
            Show {steps.length - shown.length} more step{steps.length - shown.length === 1 ? '' : 's'}
          </button>
        ) : null}
        {canCollapse ? (
          <button
            type="button"
            onClick={() => setLimit(INITIAL_STEPS)}
            className="mt-2 flex w-full items-center justify-center gap-1 rounded border border-dashed border-line-strong px-2 py-1.5 text-[12px] text-muted transition-colors hover:border-line-strong hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/50"
          >
            <ChevronsUp className="h-3 w-3" aria-hidden="true" />
            Collapse to {INITIAL_STEPS}
          </button>
        ) : null}
      </div>
    </section>
  );
}

/** One real step row: kind chip, real step id, code, inputs→output, detail, Inspect. */
function StepRow({
  step,
  position,
  index,
  onCallee,
}: {
  step: Step;
  position: number;
  index: Map<string, FnRecord>;
  onCallee: (fnId: string) => void;
}) {
  const meta = kindMeta(step.kind);
  const callee = stepCallee(step, index);
  const unresolved = Boolean(step.callsFnId && !callee);
  const inputs = step.inputs || [];

  return (
    <li className="step-spine relative pb-3 pl-8 last:pb-0" data-step-id={step.id}>
      <span
        className="absolute left-0 top-0 grid h-[22px] w-[22px] place-items-center rounded-full border border-line bg-raised font-mono text-[11px] text-muted"
        title={step.id}
      >
        {position + 1}
      </span>
      <div className="rounded-lg border border-line bg-surface p-3">
        <div className="flex flex-wrap items-center gap-1.5">
          <Badge tone={meta.tone}>{meta.label}</Badge>
          <span className="font-mono text-[11px] text-faint" title={step.id}>
            {step.id}
          </span>
          <span className="text-[13.5px] leading-5 text-ink">{step.title}</span>
        </div>
        {step.code ? <CodeBlock code={step.code} className="mt-2" /> : null}
        {inputs.length || step.output ? (
          <div className="mt-2 flex flex-wrap items-center gap-1 font-mono text-[11.5px]">
            {inputs.length ? (
              <>
                <span className="text-faint">in</span>
                {inputs.map((input, i) => (
                  <span
                    key={`${input}-${i}`}
                    className="rounded border border-line-strong bg-raised px-1.5 py-0.5 text-muted"
                  >
                    {input}
                  </span>
                ))}
              </>
            ) : null}
            {inputs.length && step.output ? (
              <ArrowRight className="h-3 w-3 text-faint" aria-hidden="true" />
            ) : null}
            {step.output ? (
              <span className="rounded border border-accent/20 bg-accent/[0.07] px-1.5 py-0.5 text-accent/90">
                {step.output}
              </span>
            ) : null}
          </div>
        ) : null}
        {step.detail ? (
          <p className="mt-1.5 text-[12.5px] leading-4 text-muted">{step.detail}</p>
        ) : null}
        {callee ? (
          <button
            type="button"
            onClick={() => onCallee(callee.id)}
            className="mt-2 inline-flex items-center gap-1 rounded border border-accent/25 bg-accent/[0.08] px-1.5 py-1 font-mono text-[12px] text-accent transition-colors hover:bg-accent/15 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/50"
          >
            Inspect {callee.name}()
            <ArrowRight className="h-3 w-3" aria-hidden="true" />
          </button>
        ) : null}
        {unresolved ? (
          <span className="mt-2 inline-flex items-center rounded border border-line-strong px-1.5 py-0.5 font-mono text-[11px] text-faint">
            unresolved: {step.callsFnId}
          </span>
        ) : null}
      </div>
    </li>
  );
}

/** Compact step list for Split; Inspect is plain selection, not a path move. */
export function CompactStepList({
  fn,
  index,
  onSelect,
}: {
  fn: FnRecord;
  index: Map<string, FnRecord>;
  onSelect: (fnId: string) => void;
}) {
  const steps = fn.steps || [];
  if (!steps.length) {
    return <InlineEmpty title="No steps extracted" detail="Nothing to show for this function." />;
  }

  return (
    <div className="space-y-2">
      {steps.map((step, i) => {
        const meta = kindMeta(step.kind);
        const callee = stepCallee(step, index);
        return (
          <div
            key={step.id || i}
            className="flex items-start gap-2 rounded-lg border border-line bg-surface p-2"
            data-step-id={step.id}
          >
            <span
              className="mt-0.5 grid h-5 w-5 shrink-0 place-items-center rounded-full border border-line bg-raised font-mono text-[10px] text-muted"
              title={step.id}
            >
              {i + 1}
            </span>
            <div className="min-w-0 flex-1">
              <div className="flex flex-wrap items-center gap-1.5">
                <span className={`font-mono text-[10px] ${TONE_TEXT[meta.tone] || TONE_TEXT.neutral}`}>
                  {meta.label}
                </span>
                <span className="text-[12.5px] text-ink">{step.title}</span>
              </div>
              {step.output ? (
                <div className="mt-0.5 font-mono text-[11px] text-muted">&rarr; {step.output}</div>
              ) : null}
              {callee ? (
                <button
                  type="button"
                  onClick={() => onSelect(callee.id)}
                  className="mt-1 font-mono text-[11px] text-accent hover:text-accent/80 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/50"
                >
                  Inspect {callee.name}() &rarr;
                </button>
              ) : null}
            </div>
          </div>
        );
      })}
    </div>
  );
}
