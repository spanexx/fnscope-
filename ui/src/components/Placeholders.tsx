import React from 'react';
import { Inbox, Search, SquareTerminal, X } from 'lucide-react';
import Badge from './Badge';
import Button from './Button';
import EmptyState from './EmptyState';
import IconButton from './IconButton';
import { ErrorState, LoadingState } from './StatusState';
import { OBJECT_ARG_MARKER } from '../api';
import { entryPointCandidates, graphSummary } from '../lib/graph';
import { formatCount } from '../lib/format';
import { runShortcutHint, searchShortcutHint } from '../lib/shortcuts';
import type { FnGraph, ScopeDescriptor, ShellStatus, ViewId } from '../lib/types';

export interface LoadError {
  message: string;
  detail?: string;
}

interface ExplorerPlaceholderProps {
  status: ShellStatus;
  scope: ScopeDescriptor | null;
  graph: FnGraph | null;
  error: LoadError | null;
  onRetry: () => void;
  searchValue: string;
  onSearchChange: (value: string) => void;
  searchRef: React.RefObject<HTMLInputElement>;
}

/** Sidebar slot: real counts now, the explorer list/module tree in Phase 4. */
export function ExplorerPlaceholder({
  status,
  scope,
  graph,
  error,
  onRetry,
  searchValue,
  onSearchChange,
  searchRef,
}: ExplorerPlaceholderProps) {
  const summary = graph ? graphSummary(graph) : null;
  const entryCount = graph ? entryPointCandidates(graph).length : 0;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="border-b border-line/70 p-3">
        <div className="mb-2 flex items-center justify-between gap-2">
          <span className="font-mono text-[9px] font-medium uppercase tracking-[0.16em] text-faint">
            Explorer
          </span>
          <span className="max-w-[150px] truncate font-mono text-[9.5px] text-faint">
            {scope ? scope.id : 'no scope'}
          </span>
        </div>
        <label className="relative block">
          <Search
            className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-faint"
            aria-hidden="true"
          />
          <input
            ref={searchRef}
            type="text"
            autoComplete="off"
            spellCheck={false}
            value={searchValue}
            onChange={(event) => onSearchChange(event.target.value)}
            disabled={status !== 'ready'}
            aria-label="Filter functions, modules and ids"
            placeholder={'Filter functions, modules, IDs\u2026'}
            className="h-8 w-full rounded-md border border-line bg-panel pl-8 pr-12 text-[11.5px] text-ink placeholder:text-faint focus:border-accent/60 focus:outline-none focus:ring-1 focus:ring-accent/20 disabled:cursor-not-allowed disabled:opacity-60"
          />
          {searchValue ? (
            <button
              type="button"
              onClick={() => onSearchChange('')}
              aria-label="Clear search"
              className="absolute right-1.5 top-1/2 -translate-y-1/2 rounded p-1 text-muted hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/50"
            >
              <X className="h-3 w-3" aria-hidden="true" />
            </button>
          ) : (
            <kbd className="pointer-events-none absolute right-2 top-1/2 -translate-y-1/2 rounded-sm border border-line bg-raised px-1 py-0.5 font-mono text-[9px] text-faint">
              {searchShortcutHint()}
            </kbd>
          )}
        </label>
        <p className="mt-1.5 font-mono text-[9px] text-faint">/ also focuses search</p>
      </div>

      <div className="border-b border-line/60 px-3 py-2 text-[10px] text-muted">
        {summary
          ? `${formatCount(summary.functionCount)} functions \u00b7 ${formatCount(summary.fileCount)} files \u00b7 ${formatCount(entryCount)} entry candidates`
          : 'Waiting for the manifest'}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto" aria-busy={status === 'loading'}>
        {status === 'loading' ? (
          <LoadingState
            label={scope ? `Loading ${scope.label}` : 'Loading workspace scopes'}
            detail="Reading the local manifest from the fnscope server."
          />
        ) : status === 'error' ? (
          <ErrorState
            compact
            title="Scope could not load"
            message={error?.message}
            detail={error?.detail}
            onRetry={onRetry}
          />
        ) : status === 'empty' ? (
          <EmptyState
            compact
            icon={<Inbox className="h-4 w-4" aria-hidden="true" />}
            title="No functions in this scope"
            description={
              <>
                <p>The manifest loaded but contains no function records.</p>
                <code className="mt-2 block rounded border border-line bg-bg px-2 py-1 font-mono text-[9.5px] text-ink">
                  fnscope scan &lt;dir&gt; --out .fnscope
                </code>
              </>
            }
          />
        ) : !scope ? (
          <EmptyState
            compact
            icon={<Inbox className="h-4 w-4" aria-hidden="true" />}
            title="No scope selected"
            description="The workspace did not list an available scope."
          />
        ) : (
          <div>
            <EmptyState
              compact
              icon={<Inbox className="h-4 w-4" aria-hidden="true" />}
              title="Explorer slot ready"
              description="The manifest is loaded. The function list, module tree, search results and selection land in the next UI job."
            />
            <dl className="mx-3 grid grid-cols-2 gap-2 border-t border-line/60 pt-3 text-[10px]">
              <div>
                <dt className="font-mono text-[9px] uppercase tracking-[0.08em] text-faint">Scope</dt>
                <dd className="truncate text-ink" title={scope.label}>
                  {scope.label}
                </dd>
              </div>
              <div>
                <dt className="font-mono text-[9px] uppercase tracking-[0.08em] text-faint">Root</dt>
                <dd className="truncate text-ink" title={scope.rootDir}>
                  {scope.rootDir || 'unknown'}
                </dd>
              </div>
              <div>
                <dt className="font-mono text-[9px] uppercase tracking-[0.08em] text-faint">Entry candidates</dt>
                <dd className="text-ink">{formatCount(entryCount)}</dd>
              </div>
              <div>
                <dt className="font-mono text-[9px] uppercase tracking-[0.08em] text-faint">Invoke</dt>
                <dd className="text-ink">{scope.invokeMode || 'unknown'}</dd>
              </div>
            </dl>
          </div>
        )}
      </div>

      <div className="border-t border-line/70 p-2.5">
        <p className="truncate text-[9.5px] leading-4 text-faint">
          {scope ? scope.rootDir || scope.label : 'No manifest root'}
        </p>
        <p className="font-mono text-[9px] uppercase tracking-[0.08em] text-faint">
          local manifest only / no sample graph
        </p>
      </div>
    </div>
  );
}

const VIEW_META: Record<ViewId, { label: string; description: string }> = {
  story: {
    label: 'Story',
    description: 'Triggered by, inputs, execution story, returns and declared errors.',
  },
  map: {
    label: '1-Hop',
    description: 'Callers on the left, the current function in the centre, callees on the right.',
  },
  split: {
    label: 'Split',
    description: 'The real source beside the execution story for the selected function.',
  },
  raw: {
    label: 'Raw',
    description: 'The complete FnRecord JSON for the selected function.',
  },
};

interface CanvasPlaceholderProps {
  view: ViewId;
  status: ShellStatus;
  scope: ScopeDescriptor | null;
  graph: FnGraph | null;
  error: LoadError | null;
  onRetry: () => void;
}

/** Canvas slot: names the active view and shows only real scope/manifest facts. */
export function CanvasPlaceholder({
  view,
  status,
  scope,
  graph,
  error,
  onRetry,
}: CanvasPlaceholderProps) {
  const meta = VIEW_META[view];
  const summary = graph ? graphSummary(graph) : null;

  if (status === 'loading') {
    return (
      <LoadingState
        label={scope ? `Loading ${scope.label} ${meta.label} view` : 'Loading workspace'}
        detail="The canvas will render real manifest data once the explorer and views land."
      />
    );
  }

  if (status === 'error') {
    return (
      <ErrorState
        title="The manifest could not be loaded"
        message={error?.message}
        detail={error?.detail}
        onRetry={onRetry}
      />
    );
  }

  if (status === 'empty' || !scope) {
    return (
      <EmptyState
        icon={<Inbox className="h-4 w-4" aria-hidden="true" />}
        title={status === 'empty' ? 'This scope is empty' : 'No scope selected'}
        description={
          status === 'empty'
            ? 'No function records exist, so there is nothing to show here. Scan a directory with fnscope before opening it.'
            : 'The workspace did not return an available scope.'
        }
      />
    );
  }

  return (
    <div className="mx-auto w-full max-w-[760px] px-5 py-8 sm:px-8">
      <div className="mb-4 flex items-center gap-2">
        <Badge tone="accent">{meta.label}</Badge>
        <Badge tone="neutral">shell placeholder</Badge>
      </div>
      <div className="rounded-lg border border-line bg-panel p-5 sm:p-6">
        <h1 className="font-serif text-[22px] font-semibold leading-tight tracking-[-0.01em] text-ink">
          {meta.label} view
        </h1>
        <p className="mt-2 max-w-[62ch] text-[12px] leading-6 text-muted">{meta.description}</p>
        <p className="mt-3 max-w-[62ch] text-[12px] leading-6 text-muted">
          The real renderer arrives in the next UI job. It will read this same loaded manifest; this
          shell never substitutes sample data.
        </p>

        <dl className="mt-6 grid grid-cols-2 gap-x-4 gap-y-3 border-t border-line pt-4 text-[11px] sm:grid-cols-3">
          <div>
            <dt className="font-mono text-[9px] uppercase tracking-[0.08em] text-faint">Scope</dt>
            <dd className="truncate text-ink" title={scope.label}>
              {scope.label}
            </dd>
          </div>
          <div>
            <dt className="font-mono text-[9px] uppercase tracking-[0.08em] text-faint">Functions</dt>
            <dd className="text-ink">{summary ? formatCount(summary.functionCount) : '\u2014'}</dd>
          </div>
          <div>
            <dt className="font-mono text-[9px] uppercase tracking-[0.08em] text-faint">Files</dt>
            <dd className="text-ink">{summary ? formatCount(summary.fileCount) : '\u2014'}</dd>
          </div>
        </dl>
      </div>
      <p className="mt-3 font-mono text-[9.5px] uppercase tracking-[0.08em] text-faint">
        Selection, story, 1-hop, split and raw renderers are pending
      </p>
    </div>
  );
}

interface InvokePlaceholderProps {
  scope: ScopeDescriptor | null;
  compatibility: boolean;
  status: ShellStatus;
  onClose: () => void;
}

/** Drawer slot: honest about no selection and the scope's real invoke mode. */
export function InvokePlaceholder({
  scope,
  compatibility,
  status,
  onClose,
}: InvokePlaceholderProps) {
  const legacy = compatibility || Boolean(scope?.compatibility);
  const route = scope
    ? legacy
      ? 'POST /api/invoke'
      : `POST /api/scopes/${encodeURIComponent(scope.id)}/invoke`
    : 'POST /api/scopes/{id}/invoke';

  const modeLabel =
    scope?.invokeMode === 'node'
      ? 'live node'
      : scope?.invokeMode === 'simulate'
        ? 'simulate'
        : 'mode unknown';
  const modeTone = scope?.invokeMode === 'node' ? 'ok' : scope?.invokeMode === 'simulate' ? 'neutral' : 'warn';

  return (
    <div
      className="flex h-full min-h-0 flex-col"
      // Keeps the TS/JS object-arg repair marker in the single built bundle
      // until Phase 6 imports repairArgsForFn directly.
      data-arg-repair-marker={OBJECT_ARG_MARKER}
    >
      <div className="flex h-11 shrink-0 items-center justify-between border-b border-line px-3">
        <div className="flex min-w-0 items-center gap-2">
          <SquareTerminal className="h-4 w-4 shrink-0 text-accent" aria-hidden="true" />
          <span className="truncate text-[12px] font-medium text-ink">Invoke sandbox</span>
          <Badge tone={modeTone}>{modeLabel}</Badge>
        </div>
        <IconButton label="Close invoke sandbox" size="sm" onClick={onClose}>
          <X className="h-3.5 w-3.5" aria-hidden="true" />
        </IconButton>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto p-3">
        {status === 'loading' ? (
          <LoadingState label="Waiting for the scope" detail="The sandbox follows the selected scope." />
        ) : status === 'error' ? (
          <EmptyState
            compact
            icon={<SquareTerminal className="h-4 w-4" aria-hidden="true" />}
            title="Scope unavailable"
            description="The drawer will unlock when the selected scope's manifest loads."
          />
        ) : (
          <EmptyState
            compact
            icon={<SquareTerminal className="h-4 w-4" aria-hidden="true" />}
            title="No function selected"
            description={
              <>
                <p>Choose a function once the explorer lands. The sandbox will post the real InvokeRequest.</p>
                <p className="mt-2 break-all font-mono text-[9.5px] text-faint">{route}</p>
              </>
            }
          />
        )}

        <div className="mt-3 rounded-md border border-line bg-bg p-2.5">
          <div className="font-mono text-[9px] uppercase tracking-[0.12em] text-faint">Route</div>
          <div className="mt-1 break-all font-mono text-[10px] text-ink">{route}</div>
        </div>

        <p className="mt-2 text-[10px] leading-4 text-muted">
          {scope?.invokeMode === 'node'
            ? 'This scope is configured for live Node execution. The drawer will label a real run honestly.'
            : scope?.invokeMode === 'simulate'
              ? 'This scope is simulate-only. The drawer will not claim a function really ran.'
              : legacy
                ? 'The legacy single-scope API does not report an invoke mode.'
                : 'Invoke mode arrives with the scope descriptor.'}
        </p>
      </div>

      <div className="shrink-0 border-t border-line p-2.5">
        <Button block disabled title="Select a function before running it">
          Run function
        </Button>
        <p className="mt-1.5 text-center font-mono text-[9px] text-faint">
          {runShortcutHint()} / select a function first
        </p>
      </div>
    </div>
  );
}
