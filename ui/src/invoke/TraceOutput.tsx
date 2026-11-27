import React from 'react';
import { Loader2, Terminal } from 'lucide-react';
import Badge from '../components/Badge';
import Button from '../components/Button';
import CodeBlock from '../components/CodeBlock';
import { ErrorState } from '../components/StatusState';
import type { FnRecord, InvokeResponse, ScopeDescriptor, Step, TraceEntry } from '../lib/types';
import type { InvokeOutcome } from './useInvoke';

interface TraceOutputProps {
  scope: ScopeDescriptor;
  fn: FnRecord;
  outcome: InvokeOutcome;
  /** Real request route, shown before the first run and on transport failure. */
  route: string;
  onRetry: () => void;
}

type StatusTone = 'neutral' | 'ok' | 'warn' | 'err';

function statusTone(status: unknown): StatusTone {
  const text = String(status ?? '').toLowerCase();
  if (/error|fail|exception|reject/.test(text)) return 'err';
  if (/skip|pending|blocked|partial|timeout/.test(text)) return 'warn';
  if (/ok|pass|success|done|complete|return|live/.test(text)) return 'ok';
  return 'neutral';
}

/** Only the server's own duration is shown; an absent value stays unknown. */
function formatDuration(ms: number): string | null {
  if (typeof ms !== 'number' || !Number.isFinite(ms)) return null;
  return `${new Intl.NumberFormat('en-US', { maximumFractionDigits: 2 }).format(ms)} ms`;
}

function safeJson(value: unknown): string {
  try {
    const text = JSON.stringify(value, null, 2);
    return text === undefined ? String(value) : text;
  } catch {
    return String(value);
  }
}

function responseLabel(scope: ScopeDescriptor): string {
  if (scope.invokeMode === 'node') return 'Live Node response';
  if (scope.invokeMode === 'simulate') return 'Simulated response';
  return 'Server response';
}

function TraceRow({
  step,
  entry,
  index,
}: {
  step: Step | null;
  entry: TraceEntry | undefined;
  index: number;
}) {
  const title = step ? step.title || step.id : entry ? 'Unlisted trace entry' : 'Trace entry';
  const detail = step ? `${step.id} \u00b7 ${step.kind}` : 'not in this manifest';
  const output =
    typeof entry?.output === 'string'
      ? entry.output
      : entry?.output === undefined || entry?.output === null
        ? ''
        : safeJson(entry.output);
  return (
    <div className="rounded-md border border-line bg-bg px-2.5 py-2">
      <div className="flex items-center gap-1.5">
        <span className="w-4 shrink-0 text-right font-mono text-[11px] text-faint">{index}</span>
        <span
          title={step ? `${step.kind}: ${step.title}` : title}
          className="min-w-0 flex-1 truncate text-[13px] text-ink"
        >
          {title}
        </span>
        {entry ? (
          entry.status ? (
            <Badge tone={statusTone(entry.status)}>{entry.status}</Badge>
          ) : (
            <Badge tone="warn">no status</Badge>
          )
        ) : (
          <Badge tone="warn" title="The server returned no entry for this manifest step">
            not traced
          </Badge>
        )}
      </div>
      <p className="mt-0.5 truncate font-mono text-[11px] text-faint" title={detail}>
        {detail}
      </p>
      {entry ? (
        entry.output ? (
          <pre className="mt-1 max-h-32 overflow-auto whitespace-pre-wrap break-words font-mono text-[12px] leading-4 text-ink">
            {entry.output}
          </pre>
        ) : (
          <p className="mt-1 text-[12px] leading-4 text-faint">No output reported.</p>
        )
      ) : null}
    </div>
  );
}

function ResponseView({
  scope,
  fn,
  outcome,
  onRetry,
}: {
  scope: ScopeDescriptor;
  fn: FnRecord;
  outcome: Extract<InvokeOutcome, { status: 'done' }>;
  onRetry: () => void;
}) {
  const response: InvokeResponse = outcome.response;
  const trace = response.trace || {};
  const manifestSteps = fn.steps || [];
  const knownIds = new Set(manifestSteps.map((step) => step.id));
  const extraIds = Object.keys(trace).filter((id) => !knownIds.has(id));
  const duration = formatDuration(response.durationMs);
  const tracedCount = Object.keys(trace).length;
  const ok = response.ok === true;

  return (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center gap-1.5">
        <Badge tone={ok ? 'ok' : 'err'}>{ok ? 'ok' : 'failed'}</Badge>
        <span className="text-[12px] text-muted">{responseLabel(scope)}</span>
        <span className="font-mono text-[11px] text-faint">
          {duration ?? 'duration not reported'}
        </span>
        <span className="font-mono text-[11px] text-faint">
          {tracedCount} {tracedCount === 1 ? 'traced step' : 'traced steps'}
        </span>
        {outcome.repaired ? (
          <Badge tone="info" title="A single inline object parameter was rebuilt from its fields">
            object arg repaired
          </Badge>
        ) : null}
      </div>

      {!ok || response.error ? (
        <div
          role="alert"
          className="rounded-md border border-err/35 bg-err/10 px-2.5 py-2"
        >
          <p className="text-[12px] font-medium text-err">
            {ok ? 'Server warning' : 'Server error'}
          </p>
          <p className="mt-0.5 break-words text-[12.5px] leading-4 text-ink">
            {response.error || 'The response failed without an error message.'}
          </p>
          <Button variant="secondary" size="sm" className="mt-1.5" onClick={onRetry}>
            Run again
          </Button>
        </div>
      ) : null}

      {response.data !== undefined ? (
        <div>
          <div className="mb-1 font-mono text-[11px] uppercase tracking-[0.12em] text-faint">
            Payload
          </div>
          <div className="max-h-[220px] overflow-auto">
            <CodeBlock code={safeJson(response.data)} language="data" />
          </div>
        </div>
      ) : (
        <p className="text-[12px] leading-4 text-faint">The response carried no data payload.</p>
      )}

      <div>
        <div className="mb-1 font-mono text-[11px] uppercase tracking-[0.12em] text-faint">
          Steps
        </div>
        {manifestSteps.length === 0 && extraIds.length === 0 ? (
          <p className="text-[12px] leading-4 text-faint">
            The server returned an empty trace for this function.
          </p>
        ) : (
          <div className="space-y-1.5">
            {manifestSteps.map((step, index) => (
              <TraceRow
                key={step.id}
                step={step}
                entry={trace[step.id]}
                index={index + 1}
              />
            ))}
            {extraIds.map((id, index) => (
              <TraceRow
                key={id}
                step={null}
                entry={trace[id]}
                index={manifestSteps.length + index + 1}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

/** Honest renderers for idle, in-flight, transport failure and server response. */
export default function TraceOutput({
  scope,
  fn,
  outcome,
  route,
  onRetry,
}: TraceOutputProps) {
  if (outcome.status === 'idle') {
    return (
      <div className="rounded-lg border border-dashed border-line-strong px-3 py-6 text-center">
        <Terminal className="mx-auto h-5 w-5 text-faint" aria-hidden="true" />
        <p className="mt-2 text-[12.5px] text-muted">
          Run the function to see the server&apos;s real trace.
        </p>
        <p className="mt-1 break-all font-mono text-[11px] text-faint">{route}</p>
      </div>
    );
  }

  if (outcome.status === 'running') {
    return (
      <div
        role="status"
        aria-live="polite"
        className="flex flex-col items-center gap-2 rounded-lg border border-line bg-bg px-3 py-6 text-center"
      >
        <Loader2 className="h-5 w-5 animate-spin text-accent" aria-hidden="true" />
        <p className="text-[12.5px] text-ink">Waiting for the server</p>
        <p className="break-all font-mono text-[11px] text-faint">{route}</p>
        <p className="text-[11px] text-faint">No duration is shown until the response arrives.</p>
      </div>
    );
  }

  if (outcome.status === 'failed') {
    return (
      <ErrorState
        compact
        title="Invoke request failed"
        message={outcome.message}
        detail={route}
        onRetry={onRetry}
      />
    );
  }

  return <ResponseView scope={scope} fn={fn} outcome={outcome} onRetry={onRetry} />;
}
