import React, { useEffect, useRef } from 'react';
import { CornerDownRight, LogIn } from 'lucide-react';
import Button from '../components/Button';
import EmptyState from '../components/EmptyState';
import { formatCount } from '../lib/format';
import type { FnGraph, FnRecord } from '../lib/types';
import { ENTRY_POINT_LIMIT, entryPointPage } from './entryPoints';
import { basename } from './grouping';

interface FunctionRowProps {
  fn: FnRecord;
  selected: boolean;
  onSelect: (id: string) => void;
  query?: string;
  indent?: boolean;
  entry?: boolean;
}

/** One selectable function row, shared by entry, search and module lists. */
export function FunctionRow({
  fn,
  selected,
  onSelect,
  query = '',
  indent = false,
  entry = false,
}: FunctionRowProps) {
  const ref = useRef<HTMLButtonElement>(null);
  const steps = (fn.steps || []).length;
  const hasErrors = (fn.errors || []).length > 0;
  const subtitle = indent
    ? `line ${fn.line} \u00b7 ${steps} steps`
    : `${fn.module || 'unknown module'} \u00b7 ${basename(fn.filePath)} \u00b7 ${steps} steps`;

  useEffect(() => {
    if (selected) ref.current?.scrollIntoView({ block: 'nearest' });
  }, [selected]);

  return (
    <button
      ref={ref}
      type="button"
      data-fn={fn.id}
      aria-current={selected ? 'true' : undefined}
      onClick={() => onSelect(fn.id)}
      title={fn.filePath}
      className={`group flex w-full items-start gap-2 border-l-2 py-2 text-left transition-colors motion-reduce:transition-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent/50 ${
        indent ? 'pl-10 pr-3' : 'px-3'
      } ${selected ? 'border-accent bg-accent/[0.09]' : 'border-transparent hover:bg-wash'}`}
    >
      <span
        className={`mt-1.5 h-1.5 w-1.5 shrink-0 rounded-full ${entry ? 'bg-accent' : 'bg-faint'}`}
        aria-hidden="true"
      />
      <span className="min-w-0 flex-1">
        <span
          className={`block truncate font-mono text-[12px] ${selected ? 'text-accent' : 'text-ink'}`}
        >
          <HighlightedName name={fn.name} query={query} />
          <span className="text-faint">()</span>
        </span>
        <span className="mt-0.5 block truncate text-[10px] text-muted">{subtitle}</span>
      </span>
      <span className="mt-1 flex shrink-0 items-center gap-1">
        {hasErrors ? (
          <span
            className="h-1.5 w-1.5 rounded-full bg-err/90"
            title="Declared error paths"
            aria-hidden="true"
          />
        ) : null}
        {entry ? (
          <CornerDownRight className="h-3 w-3 text-accent/70" aria-hidden="true" />
        ) : null}
      </span>
    </button>
  );
}

/** Bolds the matched substring without changing the row's text content. */
function HighlightedName({ name, query }: { name: string; query: string }) {
  const needle = query.trim().toLowerCase();
  const value = String(name || '');
  if (!needle) return <>{value}</>;
  const index = value.toLowerCase().indexOf(needle);
  if (index < 0) return <>{value}</>;
  return (
    <>
      {value.slice(0, index)}
      <mark className="rounded-sm bg-accent/20 text-inherit">
        {value.slice(index, index + needle.length)}
      </mark>
      {value.slice(index + needle.length)}
    </>
  );
}

interface FunctionListProps {
  graph: FnGraph;
  selectedId: string | null;
  onSelect: (id: string) => void;
  showAll: boolean;
  onToggleShowAll: () => void;
  query?: string;
}

/** Entry-point candidates with the 24 cap and an explicit show-all control. */
export default function FunctionList({
  graph,
  selectedId,
  onSelect,
  showAll,
  onToggleShowAll,
  query = '',
}: FunctionListProps) {
  const { items, total, hidden } = entryPointPage(graph, showAll);

  return (
    <div>
      <div className="flex items-center justify-between px-3 pb-1 pt-2">
        <span className="text-[10px] font-semibold uppercase tracking-[0.12em] text-muted">
          Entry points
        </span>
        <span className="font-mono text-[9.5px] text-faint">{formatCount(total)}</span>
      </div>
      <p className="px-3 pb-2 text-[10px] leading-4 text-faint">
        Zero-caller functions, ranked by step count
      </p>

      {items.map((fn) => (
        <FunctionRow
          key={fn.id}
          fn={fn}
          selected={fn.id === selectedId}
          onSelect={onSelect}
          query={query}
          entry
        />
      ))}

      {total === 0 ? (
        <EmptyState
          compact
          icon={<LogIn className="h-4 w-4" aria-hidden="true" />}
          title="No entry points"
          description="Every function in this scope has a resolved caller."
        />
      ) : null}

      {hidden > 0 ? (
        <div className="border-t border-line/60 px-3 py-2">
          <p className="pb-1.5 text-[9.5px] leading-4 text-faint">
            {showAll
              ? `Showing all ${formatCount(total)} candidates.`
              : `${formatCount(hidden)} more zero-caller functions are searchable but not listed.`}
          </p>
          <Button variant="ghost" size="sm" block onClick={onToggleShowAll}>
            {showAll
              ? `Show first ${formatCount(Math.min(ENTRY_POINT_LIMIT, total))}`
              : `Show all ${formatCount(total)}`}
          </Button>
        </div>
      ) : null}
    </div>
  );
}
