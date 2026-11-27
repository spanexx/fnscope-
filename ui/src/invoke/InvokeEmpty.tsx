import React from 'react';
import { Inbox, Play, SquareTerminal, X } from 'lucide-react';
import Badge from '../components/Badge';
import Button from '../components/Button';
import EmptyState from '../components/EmptyState';
import IconButton from '../components/IconButton';
import { LoadingState } from '../components/StatusState';
import { runShortcutHint } from '../lib/shortcuts';
import type { ScopeDescriptor, ShellStatus } from '../lib/types';

interface InvokeEmptyProps {
  scope: ScopeDescriptor | null;
  compatibility: boolean;
  status: ShellStatus;
  onClose: () => void;
}

/**
 * Screens the sandbox shows before it has a real function: same frame and mode
 * badge as a run, but no arguments, no trace and an explicit reason.
 */
export default function InvokeEmpty({
  scope,
  compatibility,
  status,
  onClose,
}: InvokeEmptyProps) {
  const legacy = compatibility || Boolean(scope?.compatibility);
  const route = scope
    ? legacy
      ? 'POST /api/invoke'
      : `POST /api/scopes/${encodeURIComponent(scope.id)}/invoke`
    : 'POST /api/scopes/{id}/invoke';

  const modeLabel =
    scope?.invokeMode === 'node'
      ? 'Live Node'
      : scope?.invokeMode === 'simulate'
        ? 'Simulate'
        : legacy
          ? 'Legacy API'
          : 'Mode unknown';
  const modeTone =
    scope?.invokeMode === 'node' ? 'ok' : scope?.invokeMode === 'simulate' ? 'neutral' : 'warn';

  const empty = (() => {
    if (status === 'loading') {
      return (
        <LoadingState
          label={scope ? `Loading ${scope.label}` : 'Waiting for the scope'}
          detail="The sandbox follows the selected scope and function."
        />
      );
    }
    if (status === 'error') {
      return (
        <EmptyState
          compact
          icon={<SquareTerminal className="h-4 w-4" aria-hidden="true" />}
          title="Scope unavailable"
          description="The sandbox unlocks when the selected scope's manifest loads."
        />
      );
    }
    if (!scope) {
      return (
        <EmptyState
          compact
          icon={<Inbox className="h-4 w-4" aria-hidden="true" />}
          title="No scope selected"
          description="The workspace did not list an available scope."
        />
      );
    }
    if (status === 'empty') {
      return (
        <EmptyState
          compact
          icon={<Inbox className="h-4 w-4" aria-hidden="true" />}
          title="No functions in this scope"
          description="The manifest loaded but contains no function records to run."
        />
      );
    }
    return (
      <EmptyState
        compact
        icon={<SquareTerminal className="h-4 w-4" aria-hidden="true" />}
        title="No function selected"
        description="Choose a function in the explorer to build its real argument template."
      />
    );
  })();

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex h-11 shrink-0 items-center justify-between gap-2 border-b border-line px-3">
        <div className="flex min-w-0 items-center gap-2">
          <SquareTerminal className="h-4 w-4 shrink-0 text-accent" aria-hidden="true" />
          <span className="truncate text-[14px] font-medium text-ink">Invoke sandbox</span>
          <Badge tone={modeTone}>{modeLabel}</Badge>
        </div>
        <IconButton label="Close invoke sandbox" size="sm" onClick={onClose}>
          <X className="h-3.5 w-3.5" aria-hidden="true" />
        </IconButton>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto p-3">
        {empty}

        <div className="mt-3 rounded-md border border-line bg-bg p-2.5">
          <div className="font-mono text-[11px] uppercase tracking-[0.12em] text-faint">Route</div>
          <div className="mt-1 break-all font-mono text-[12px] text-ink">{route}</div>
        </div>

        <p className="mt-2 text-[12px] leading-4 text-muted">
          {scope?.invokeMode === 'node'
            ? 'This scope is configured for live Node execution; a run posts the real request.'
            : scope?.invokeMode === 'simulate'
              ? 'This scope is simulate-only. A run returns the server’s simulated trace and says so.'
              : legacy
                ? 'The legacy single-scope API does not report an invoke mode; the response is shown as returned.'
                : 'Invoke mode arrives with the scope descriptor.'}
        </p>
      </div>

      <div className="shrink-0 border-t border-line p-2.5">
        <Button block disabled title="Select a function before running it">
          <Play className="h-3.5 w-3.5" aria-hidden="true" />
          Run function
        </Button>
        <p className="mt-1.5 text-center font-mono text-[11px] text-faint">
          {runShortcutHint()} / select a function first
        </p>
      </div>
    </div>
  );
}
