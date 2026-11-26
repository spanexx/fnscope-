import React, { useEffect, useState } from 'react';
import { FolderTree, Inbox, LogIn, SearchX } from 'lucide-react';
import EmptyState from '../components/EmptyState';
import { ErrorState, LoadingState } from '../components/StatusState';
import { formatCount } from '../lib/format';
import { graphSummary } from '../lib/graph';
import type { FnGraph, ScopeDescriptor, ShellStatus } from '../lib/types';
import FunctionList, { FunctionRow } from './FunctionList';
import ModuleTree from './ModuleTree';
import Search from './Search';
import { entryPointCount, searchFunctions } from './entryPoints';

/** Structural match for the shell's load error; avoids importing placeholder code. */
export interface ExplorerError {
  message: string;
  detail?: string;
}

export interface ExplorerProps {
  graph: FnGraph | null;
  selectedId: string | null;
  onSelect: (id: string) => void;
  search: string;
  onSearchChange: (value: string) => void;
  searchRef: React.RefObject<HTMLInputElement>;
  status: ShellStatus;
  scope: ScopeDescriptor | null;
  error: ExplorerError | null;
  onRetry: () => void;
}

type ExplorerMode = 'entries' | 'modules';

/**
 * Sidebar explorer: same frame as the shell placeholder, backed by real
 * manifest data. Selection, trail and search live in App; this only renders.
 */
export default function Explorer({
  graph,
  selectedId,
  onSelect,
  search,
  onSearchChange,
  searchRef,
  status,
  scope,
  error,
  onRetry,
}: ExplorerProps) {
  const [mode, setMode] = useState<ExplorerMode>('entries');
  const [showAll, setShowAll] = useState(false);
  const query = search.trim();

  useEffect(() => {
    setShowAll(false);
  }, [search]);

  const summary = graph ? graphSummary(graph) : null;
  const entries = entryPointCount(graph);
  const matchResult = graph && query ? searchFunctions(graph, query) : null;
  const ready = status === 'ready' && Boolean(graph);

  const switchMode = (next: ExplorerMode) => {
    setMode(next);
  };

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="border-b border-line/70 p-3">
        <div className="mb-2 flex items-center justify-between gap-2">
          <span className="font-mono text-[9px] font-medium uppercase tracking-[0.16em] text-faint">
            Explorer
          </span>
          <span
            className="max-w-[150px] truncate font-mono text-[9.5px] text-faint"
            title={scope?.label}
          >
            {scope ? scope.id : 'no scope'}
          </span>
        </div>

        <Search
          value={search}
          onChange={onSearchChange}
          inputRef={searchRef}
          disabled={!ready}
        />

        <div
          role="group"
          aria-label="Explorer mode"
          className="mt-2 flex rounded-md border border-line bg-raised/60 p-0.5"
        >
          <ModeButton
            active={mode === 'entries'}
            disabled={!ready}
            onClick={() => switchMode('entries')}
            icon={<LogIn className="h-3 w-3" aria-hidden="true" />}
          >
            Entry points
          </ModeButton>
          <ModeButton
            active={mode === 'modules'}
            disabled={!ready}
            onClick={() => switchMode('modules')}
            icon={<FolderTree className="h-3 w-3" aria-hidden="true" />}
          >
            Module tree
          </ModeButton>
        </div>
      </div>

      <div className="border-b border-line/60 px-3 py-2 text-[10px] text-muted">
        {summary
          ? `${formatCount(summary.functionCount)} functions \u00b7 ${formatCount(
              summary.fileCount
            )} files \u00b7 ${formatCount(entries)} entry candidates`
          : 'Waiting for the manifest'}
      </div>

      <div
        role="region"
        aria-label="Explorer results"
        className="min-h-0 flex-1 overflow-y-auto"
        aria-busy={status === 'loading'}
      >
        {status === 'loading' ? (
          <LoadingState
            label={scope ? `Loading ${scope.label}` : 'Loading workspace scopes'}
            detail="Reading the local manifest from the workspace server."
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
            description="The manifest loaded but contains no function records."
          />
        ) : !scope ? (
          <EmptyState
            compact
            icon={<Inbox className="h-4 w-4" aria-hidden="true" />}
            title="No scope selected"
            description="The workspace did not list an available scope."
          />
        ) : !graph ? (
          <EmptyState
            compact
            icon={<Inbox className="h-4 w-4" aria-hidden="true" />}
            title="Manifest unavailable"
            description="The selected scope has no loaded graph."
          />
        ) : matchResult ? (
          <div role="group" aria-label="Search results">
            <div className="flex items-center justify-between px-3 pb-1 pt-2">
              <span className="text-[10px] font-semibold uppercase tracking-[0.12em] text-muted">
                Search
              </span>
              <span className="font-mono text-[9.5px] text-faint">
                {formatCount(matchResult.total)} match{matchResult.total === 1 ? '' : 'es'}
              </span>
            </div>
            {matchResult.matches.map((fn) => (
              <FunctionRow
                key={fn.id}
                fn={fn}
                selected={fn.id === selectedId}
                onSelect={onSelect}
                query={query}
              />
            ))}
            {matchResult.total > matchResult.matches.length ? (
              <p className="px-3 py-2 text-[10px] leading-4 text-faint">
                Showing {formatCount(matchResult.matches.length)} of{' '}
                {formatCount(matchResult.total)}. Narrow the filter to see more.
              </p>
            ) : null}
            {matchResult.total === 0 ? (
              <EmptyState
                compact
                icon={<SearchX className="h-4 w-4" aria-hidden="true" />}
                title="No functions match"
                description={`Nothing contains \u201c${query}\u201d in this scope.`}
              />
            ) : null}
          </div>
        ) : mode === 'modules' ? (
          <ModuleTree graph={graph} selectedId={selectedId} onSelect={onSelect} />
        ) : (
          <FunctionList
            graph={graph}
            selectedId={selectedId}
            onSelect={onSelect}
            showAll={showAll}
            onToggleShowAll={() => setShowAll((value) => !value)}
          />
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

interface ModeButtonProps {
  active: boolean;
  disabled: boolean;
  onClick: () => void;
  icon: React.ReactNode;
  children: React.ReactNode;
}

/** Segmented control button; aria-pressed carries the mode state. */
function ModeButton({ active, disabled, onClick, icon, children }: ModeButtonProps) {
  return (
    <button
      type="button"
      aria-pressed={active}
      disabled={disabled}
      onClick={onClick}
      className={`flex flex-1 items-center justify-center gap-1 rounded px-1.5 py-1 text-[10px] transition-colors motion-reduce:transition-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/50 disabled:cursor-not-allowed disabled:opacity-50 ${
        active ? 'bg-panel text-ink shadow-sm' : 'text-muted hover:text-ink'
      }`}
    >
      {icon}
      {children}
    </button>
  );
}
