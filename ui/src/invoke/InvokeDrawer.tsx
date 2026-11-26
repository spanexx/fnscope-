import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { FlaskConical, Loader2, Play, RotateCcw, SquareTerminal, X, Zap } from 'lucide-react';
import Badge from '../components/Badge';
import Button from '../components/Button';
import IconButton from '../components/IconButton';
import { runShortcutHint } from '../lib/shortcuts';
import type { FnRecord, ScopeDescriptor } from '../lib/types';
import ArgsEditor from './ArgsEditor';
import TraceOutput from './TraceOutput';
import { buildArgsTemplate, formatSignature, parameterSummary, parseArgsInput } from './args';
import type { ArgsError } from './args';
import { useInvoke } from './useInvoke';

export interface InvokeDrawerProps {
  scope: ScopeDescriptor;
  fn: FnRecord;
  onClose: () => void;
  /** True when the legacy single-scope API is serving, which has its own route. */
  compatibility?: boolean;
}

type ModeTone = 'neutral' | 'ok' | 'warn';

interface ModeView {
  label: string;
  tone: ModeTone;
  runLabel: string;
  note: string;
}

/**
 * The server decides the mode; the drawer only labels it. There is no live
 * toggle here because no request field asks the server to execute source.
 */
function modeView(scope: ScopeDescriptor, legacy: boolean): ModeView {
  if (scope.invokeMode === 'node') {
    return {
      label: 'Live Node',
      tone: 'ok',
      runLabel: 'Run live',
      note: 'This scope is configured for live Node execution. The trace below is the runner\u2019s real response.',
    };
  }
  if (scope.invokeMode === 'simulate') {
    return {
      label: 'Simulate',
      tone: 'neutral',
      runLabel: 'Simulate run',
      note: 'This scope is simulate-only. No source executes, and every response is labelled simulated.',
    };
  }
  return {
    label: legacy ? 'Legacy API' : 'Mode unknown',
    tone: 'warn',
    runLabel: 'Run function',
    note: legacy
      ? 'The legacy single-scope API does not report an invoke mode; the response is shown exactly as returned.'
      : 'The server did not report an invoke mode, so the drawer cannot promise a live run.',
  };
}

function invokeRoute(scope: ScopeDescriptor, legacy: boolean): string {
  if (legacy) return 'POST /api/invoke';
  return `POST /api/scopes/${encodeURIComponent(scope.id)}/invoke`;
}

/** Real arguments in, real InvokeResponse out; nothing is timed or faked here. */
export default function InvokeDrawer({
  scope,
  fn,
  onClose,
  compatibility = false,
}: InvokeDrawerProps) {
  const legacy = compatibility || Boolean(scope.compatibility);
  const mode = modeView(scope, legacy);
  const route = invokeRoute(scope, legacy);
  const template = useMemo(() => buildArgsTemplate(fn), [fn]);
  const summary = useMemo(() => parameterSummary(fn.parameters), [fn.parameters]);
  const signature = useMemo(() => formatSignature(fn), [fn]);

  const [text, setText] = useState(template.text);
  const [error, setError] = useState<ArgsError | null>(null);
  const { outcome, running, run, reset } = useInvoke(scope, fn);

  // A new function or scope owns a fresh template and an empty trace.
  useEffect(() => {
    setText(template.text);
    setError(null);
  }, [template]);

  const handleChange = useCallback((next: string) => {
    setText(next);
    setError(null);
  }, []);

  const handleRun = useCallback(async () => {
    if (running) return;
    const parsed = parseArgsInput(text);
    if (!parsed.ok) {
      setError(parsed.error);
      return;
    }
    setError(null);
    await run(parsed.value);
  }, [running, text, run]);

  const handleReset = useCallback(() => {
    setText(template.text);
    setError(null);
    reset();
  }, [template, reset]);

  const handleKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLDivElement>) => {
      if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
        event.preventDefault();
        event.stopPropagation();
        void handleRun();
      }
    },
    [handleRun]
  );

  const runDisabled = running || !scope.available;
  const runTitle = !scope.available
    ? 'This scope is unavailable'
    : running
      ? 'A request is already in flight'
      : scope.invokeMode === 'simulate'
        ? 'Send the real invoke request; the server returns a simulated trace'
        : scope.invokeMode === 'node'
          ? 'Execute the function in the scope\u2019s Node runner'
          : 'Send the invoke request';

  const liveTitle =
    scope.invokeMode === 'node'
      ? 'This scope executes the function in its Node runtime.'
      : scope.invokeMode === 'simulate'
        ? 'This scope is simulate-only; the server never executes source, so a live run is not available.'
        : 'The server did not report an invoke mode; live execution cannot be promised.';
  const simulateTitle =
    scope.invokeMode === 'simulate'
      ? 'The server returns a simulated trace for this scope.'
      : scope.invokeMode === 'node'
        ? 'This scope runs live source; the server does not offer a separate simulation route.'
        : 'The server did not report an invoke mode; simulation cannot be promised.';

  return (
    <div className="flex h-full min-h-0 flex-col" onKeyDown={handleKeyDown}>
      <div className="shrink-0 border-b border-line px-3 pb-2">
        <div className="flex h-11 items-center justify-between gap-2">
          <div className="flex min-w-0 items-center gap-2">
            <SquareTerminal className="h-4 w-4 shrink-0 text-accent" aria-hidden="true" />
            <span className="truncate text-[12px] font-medium text-ink">Invoke sandbox</span>
            <Badge tone={mode.tone}>{mode.label}</Badge>
          </div>
          <IconButton label="Close invoke sandbox" size="sm" onClick={onClose}>
            <X className="h-3.5 w-3.5" aria-hidden="true" />
          </IconButton>
        </div>

        <div className="flex min-w-0 items-baseline gap-1.5">
          <span className="truncate font-mono text-[11.5px] text-ink" title={signature}>
            {fn.name}()
          </span>
          <span className="shrink-0 font-mono text-[9px] text-faint">
            {fn.runtime || 'runtime unknown'}
          </span>
        </div>
        <p
          className="truncate font-mono text-[9.5px] text-muted"
          title={`${scope.label} \u00b7 ${fn.module}`}
        >
          {scope.label} {'\u00b7'} {fn.module}
        </p>
        <p className="mt-0.5 truncate font-mono text-[9px] text-faint" title={signature}>
          {signature}
        </p>
      </div>

      <div className="min-h-0 flex-1 space-y-3 overflow-y-auto p-3">
        <div
          role="group"
          aria-label="Server invoke mode"
          className="flex items-center gap-1 rounded-md border border-line bg-bg p-0.5"
        >
          <span title={simulateTitle} className="flex-1">
            <button
              type="button"
              disabled={scope.invokeMode !== 'simulate'}
              aria-pressed={scope.invokeMode === 'simulate'}
              title={simulateTitle}
              className={`flex w-full items-center justify-center gap-1 rounded px-2 py-1 text-[10px] transition-colors ${
                scope.invokeMode === 'simulate'
                  ? 'bg-active text-ink shadow-sm'
                  : 'cursor-not-allowed text-faint'
              }`}
            >
              <FlaskConical className="h-3 w-3" aria-hidden="true" />
              Simulate
            </button>
          </span>
          <span title={liveTitle} className="flex-1">
            <button
              type="button"
              disabled={scope.invokeMode !== 'node'}
              aria-pressed={scope.invokeMode === 'node'}
              title={liveTitle}
              className={`flex w-full items-center justify-center gap-1 rounded px-2 py-1 text-[10px] transition-colors ${
                scope.invokeMode === 'node'
                  ? 'bg-active text-ink shadow-sm'
                  : 'cursor-not-allowed text-faint'
              }`}
            >
              <Zap className="h-3 w-3" aria-hidden="true" />
              Live Node
            </button>
          </span>
        </div>

        <p className="text-[10px] leading-4 text-muted">{mode.note}</p>

        <ArgsEditor
          text={text}
          error={error}
          summary={summary}
          onChange={handleChange}
        />

        <div className="flex gap-2">
          <Button
            variant="primary"
            block
            onClick={() => void handleRun()}
            disabled={runDisabled}
            title={runTitle}
          >
            {running ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
            ) : (
              <Play className="h-3.5 w-3.5" aria-hidden="true" />
            )}
            {running ? 'Running\u2026' : mode.runLabel}
          </Button>
          <Button
            variant="secondary"
            onClick={handleReset}
            title="Restore the manifest template and clear the trace"
          >
            <RotateCcw className="h-3 w-3" aria-hidden="true" />
            Reset
          </Button>
        </div>
        <p className="text-[9px] leading-4 text-faint">
          {runShortcutHint()} runs {'\u00b7'} blank arguments send {'{}'}
        </p>

        <div>
          <div className="mb-1.5 flex items-center justify-between gap-2">
            <span className="text-[10px] font-medium uppercase tracking-[0.12em] text-muted">
              Trace output
            </span>
            <span className="truncate font-mono text-[9px] text-faint" title={route}>
              {route}
            </span>
          </div>
          <TraceOutput
            scope={scope}
            fn={fn}
            outcome={outcome}
            route={route}
            onRetry={() => void handleRun()}
          />
        </div>

        <div className="border-t border-line/70 pt-2 text-[9px] leading-4 text-faint">
          {scope.invokeMode === 'node'
            ? "Live runs execute source on this machine through the scope's Node runner."
            : scope.invokeMode === 'simulate'
              ? 'Simulate mode does not execute source; it builds the trace the server returns.'
              : 'The invoke mode is unknown until the server reports one.'}
        </div>
      </div>
    </div>
  );
}
