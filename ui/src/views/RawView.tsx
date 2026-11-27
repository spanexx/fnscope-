import React from 'react';
import { Copy } from 'lucide-react';
import CodeBlock from '../components/CodeBlock';
import { useToast } from '../components/Toast';
import type { CanvasViewProps } from './props';

/** Raw canvas: the complete, untrimmed FnRecord serialized as pretty JSON. */
export default function RawView({ fn }: CanvasViewProps) {
  const { push } = useToast();

  if (!fn) return null;
  const json = JSON.stringify(fn, null, 2);

  async function copyJson() {
    try {
      await navigator.clipboard.writeText(json);
      push('JSON copied to clipboard', 'success');
    } catch {
      push('Clipboard is unavailable in this browser', 'error');
    }
  }

  return (
    <div className="mx-auto w-full max-w-[1000px] px-5 py-5 sm:px-7">
      <div className="flex flex-wrap items-end justify-between gap-2">
        <div>
          <span className="text-[12px] font-semibold uppercase tracking-[0.14em] text-muted">
            Raw manifest record
          </span>
          <div className="mt-1 font-mono text-[18px] text-ink">{fn.name}()</div>
        </div>
        <span className="font-mono text-[11.5px] text-muted">{fn.id}</span>
      </div>

      <div className="mt-3 overflow-hidden rounded-xl border border-line bg-surface">
        <div className="flex items-center justify-between border-b border-line px-3 py-2">
          <span className="font-mono text-[12px] text-muted">{fn.id}.json</span>
          <button
            type="button"
            onClick={copyJson}
            className="inline-flex items-center gap-1 rounded border border-line-strong px-1.5 py-1 text-[11.5px] text-muted transition-colors hover:border-line-strong hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/50"
          >
            <Copy className="h-2.5 w-2.5" aria-hidden="true" />
            Copy JSON
          </button>
        </div>
        <div className="max-h-[72vh] overflow-auto">
          <CodeBlock code={json} className="!rounded-none !border-0" />
        </div>
      </div>

      <p className="mt-2 text-[12px] text-faint">
        Complete record from the served manifest; Copy JSON puts this exact text on the clipboard.
      </p>
    </div>
  );
}
