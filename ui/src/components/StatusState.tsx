import React from 'react';
import { AlertTriangle, Loader2, RefreshCw } from 'lucide-react';
import Button from './Button';
import EmptyState from './EmptyState';

/** Named loading state so a slow manifest never looks like an empty scope. */
export function LoadingState({ label, detail }: { label: string; detail?: string }) {
  return (
    <div role="status" aria-live="polite" className="flex flex-col items-center justify-center gap-2 px-6 py-12 text-center">
      <Loader2 className="h-5 w-5 animate-spin text-accent" aria-hidden="true" />
      <p className="text-[12px] font-medium text-ink">{label}</p>
      {detail ? <p className="max-w-[40ch] text-[11px] leading-5 text-muted">{detail}</p> : null}
    </div>
  );
}

interface ErrorStateProps {
  title: string;
  message?: string;
  detail?: string;
  onRetry?: () => void;
  compact?: boolean;
}

/** Recoverable error with the real server/API detail and a retry action. */
export function ErrorState({ title, message, detail, onRetry, compact = false }: ErrorStateProps) {
  return (
    <EmptyState
      compact={compact}
      role="alert"
      icon={<AlertTriangle className="h-4 w-4 text-err" aria-hidden="true" />}
      title={title}
      description={
        <>
          {message ? <p>{message}</p> : null}
          {detail ? <p className="mt-1 break-words font-mono text-[10px] text-faint">{detail}</p> : null}
        </>
      }
      action={
        onRetry ? (
          <Button variant="secondary" onClick={onRetry}>
            <RefreshCw className="h-3 w-3" aria-hidden="true" />
            Retry
          </Button>
        ) : undefined
      }
    />
  );
}
