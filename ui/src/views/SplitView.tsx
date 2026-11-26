import React from 'react';
import { FileCode, Route } from 'lucide-react';
import Badge from '../components/Badge';
import CodeBlock from '../components/CodeBlock';
import { indexFunctions, languageForPath } from '../lib/graph';
import { signature } from './helpers';
import { CompactStepList } from './StepList';
import type { CanvasViewProps } from './props';

/** Split canvas: the full real source beside the compact execution story. */
export default function SplitView({ graph, fn, onSelect }: CanvasViewProps) {
  if (!fn) return null;
  const index = indexFunctions(graph);
  const language = languageForPath(fn.filePath);

  return (
    <div className="mx-auto w-full max-w-[1200px] px-5 py-5 sm:px-7">
      <div className="flex flex-wrap items-end justify-between gap-2">
        <div>
          <div className="flex items-center gap-2">
            <span className="text-[10px] font-semibold uppercase tracking-[0.14em] text-muted">
              Split &middot; source + story
            </span>
            <Badge tone="neutral">{language}</Badge>
          </div>
          <div className="mt-1 break-words font-mono text-[16px] text-ink">{signature(fn)}</div>
        </div>
        <span className="font-mono text-[9.5px] text-muted">
          {fn.filePath}:{fn.line}
        </span>
      </div>

      <div className="mt-3 grid gap-3 lg:grid-cols-2">
        <section className="overflow-hidden rounded-xl border border-line bg-surface">
          <div className="flex items-center gap-2 border-b border-line px-3 py-2">
            <FileCode className="h-3.5 w-3.5 text-info" aria-hidden="true" />
            <span className="text-[11px] text-ink">Source</span>
            <span className="ml-auto font-mono text-[9px] text-faint">{fn.filePath}</span>
          </div>
          {fn.sourceCode ? (
            <div className="max-h-[70vh] overflow-auto">
              <CodeBlock code={fn.sourceCode} language={language} />
            </div>
          ) : (
            <div className="p-8 text-center">
              <p className="text-[11px] text-muted">The manifest has no sourceCode for this function.</p>
              <p className="mt-1 font-mono text-[9.5px] text-faint">
                {fn.filePath}:{fn.line}
              </p>
            </div>
          )}
        </section>

        <section className="rounded-xl border border-line bg-panel">
          <div className="flex items-center gap-2 border-b border-line px-3 py-2">
            <Route className="h-3.5 w-3.5 text-accent" aria-hidden="true" />
            <span className="text-[11px] text-ink">Execution story</span>
            <span className="ml-auto font-mono text-[9px] text-faint">{fn.steps.length} steps</span>
          </div>
          <div className="p-3">
            <CompactStepList fn={fn} index={index} onSelect={onSelect} />
          </div>
        </section>
      </div>
    </div>
  );
}
