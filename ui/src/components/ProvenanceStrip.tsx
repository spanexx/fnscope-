import React from 'react';
import { Shield } from 'lucide-react';
import Badge from './Badge';
import { formatCount } from '../lib/format';
import { formatGeneratedAt, graphSummary } from '../lib/graph';
import type { FnGraph, ScopeDescriptor, ShellStatus } from '../lib/types';

interface ProvenanceStripProps {
  status: ShellStatus;
  scope: ScopeDescriptor | null;
  graph: FnGraph | null;
  compatibility: boolean;
}

/** Footer proving where the data came from and what mode is active. */
export default function ProvenanceStrip({
  status,
  scope,
  graph,
  compatibility,
}: ProvenanceStripProps) {
  const summary = graph ? graphSummary(graph) : null;
  const invokeLabel =
    scope?.invokeMode === 'node'
      ? 'invoke: live Node'
      : scope?.invokeMode === 'simulate'
        ? 'invoke: simulate'
        : scope
          ? 'invoke: unknown'
          : 'invoke: \u2014';

  return (
    <footer className="ws-provenance" aria-label="Data provenance">
      <span className="inline-flex shrink-0 items-center gap-1 text-muted">
        <Shield className="h-3 w-3 text-ok" aria-hidden="true" />
        Local only
      </span>
      <span className="hidden h-3 w-px shrink-0 bg-line sm:block" />
      <span className="min-w-0 truncate" title={scope?.rootDir || undefined}>
        root: {scope?.rootDir || '\u2014'}
      </span>
      {summary ? (
        <>
          <span className="hidden h-3 w-px shrink-0 bg-line sm:block" />
          <span className="shrink-0">
            {formatCount(summary.functionCount)} fns \u00b7 {formatCount(summary.fileCount)} files
          </span>
        </>
      ) : null}
      {graph ? (
        <>
          <span className="hidden h-3 w-px shrink-0 bg-line md:block" />
          <span className="hidden shrink-0 md:inline">
            generated {formatGeneratedAt(graph.generatedAt)}
          </span>
        </>
      ) : null}
      <span className="ml-auto hidden h-3 w-px shrink-0 bg-line sm:block" />
      <span className="shrink-0">{invokeLabel}</span>
      {status === 'loading' ? <Badge tone="neutral">loading</Badge> : null}
      {status === 'error' ? <Badge tone="err">load error</Badge> : null}
      {status === 'empty' ? <Badge tone="warn">empty</Badge> : null}
      {compatibility ? <Badge tone="warn">legacy single-scope API</Badge> : null}
    </footer>
  );
}
