import React from 'react';
import { CornerUpLeft, FileCode, Hash, ListOrdered, Play, Copy } from 'lucide-react';
import Badge from '../components/Badge';
import Button from '../components/Button';
import { useToast } from '../components/Toast';
import { isEntryPointCandidate, languageForPath } from '../lib/graph';
import type { FnRecord } from '../lib/types';
import { moduleLabel, signature } from './helpers';

interface HeaderCardProps {
  fn: FnRecord;
  onInvoke: () => void;
}

/** Story header: every badge, signature and count comes from the selected record. */
export default function HeaderCard({ fn, onInvoke }: HeaderCardProps) {
  const { push } = useToast();
  const language = languageForPath(fn.filePath);
  const pure = fn.purity === 'pure';
  const entry = isEntryPointCandidate(fn);
  const callerCount = (fn.calledBy || []).length;

  async function copySignature() {
    try {
      await navigator.clipboard.writeText(signature(fn));
      push('Signature copied to clipboard', 'success');
    } catch {
      push('Clipboard is unavailable in this browser', 'error');
    }
  }

  return (
    <section className="overflow-hidden rounded-xl border border-line bg-panel">
      <div className="h-[3px] bg-accent" />
      <div className="p-4 lg:p-5">
        <div className="flex flex-wrap items-center gap-1.5">
          <Badge tone="neutral" title={fn.module}>
            {moduleLabel(fn.module)}
          </Badge>
          <Badge tone="neutral">{language}</Badge>
          <Badge tone={pure ? 'ok' : 'warn'}>{pure ? 'pure' : 'side-effect'}</Badge>
          {entry ? <Badge tone="accent">entry point</Badge> : null}
        </div>
        <h1 className="mt-3 break-words font-mono text-[19px] leading-7 text-ink lg:text-[22px]">
          {signature(fn)}
        </h1>
        <p className="mt-2 max-w-[78ch] text-[15px] leading-6 text-muted">
          {fn.description || 'No description in the manifest.'}
        </p>
        <div className="mt-3 flex flex-wrap items-center gap-x-4 gap-y-1.5 font-mono text-[12.5px] text-muted">
          <span className="inline-flex items-center gap-1">
            <FileCode className="h-3 w-3" aria-hidden="true" />
            {fn.filePath}:{fn.line}
          </span>
          <span className="inline-flex items-center gap-1">
            <Hash className="h-3 w-3" aria-hidden="true" />
            {fn.id}
          </span>
          <span className="inline-flex items-center gap-1">
            <ListOrdered className="h-3 w-3" aria-hidden="true" />
            {fn.steps.length} steps
          </span>
          <span className="inline-flex items-center gap-1">
            <CornerUpLeft className="h-3 w-3" aria-hidden="true" />
            {callerCount} callers
          </span>
        </div>
        <div className="mt-4 flex flex-wrap gap-2">
          <Button variant="primary" onClick={onInvoke}>
            <Play className="h-3 w-3" aria-hidden="true" />
            Invoke
          </Button>
          <Button variant="secondary" onClick={copySignature}>
            <Copy className="h-3 w-3" aria-hidden="true" />
            Copy signature
          </Button>
        </div>
      </div>
    </section>
  );
}
