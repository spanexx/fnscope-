import React, { useEffect, useId, useRef } from 'react';
import type { ArgsError } from './args';

interface ArgsEditorProps {
  text: string;
  error: ArgsError | null;
  /** Real parameter legend shown when the input is valid. */
  summary: string;
  disabled?: boolean;
  onChange: (text: string) => void;
}

/**
 * JSON argument editor. A parse failure re-focuses the field and drops the
 * caret at the reported offset, so the fix needs no hunting for the line.
 */
export default function ArgsEditor({
  text,
  error,
  summary,
  disabled = false,
  onChange,
}: ArgsEditorProps) {
  const fieldRef = useRef<HTMLTextAreaElement>(null);
  const fieldId = useId();
  const errorId = `${fieldId}-error`;
  const summaryId = `${fieldId}-summary`;

  useEffect(() => {
    if (!error) return;
    const field = fieldRef.current;
    if (!field) return;
    field.focus();
    if (error.position !== null && error.position >= 0) {
      const at = Math.min(error.position, field.value.length);
      field.setSelectionRange(at, at);
    }
  }, [error]);

  return (
    <div>
      <div className="mb-1.5 flex items-center justify-between gap-2">
        <label
          htmlFor={fieldId}
          className="text-[10px] font-medium uppercase tracking-[0.12em] text-muted"
        >
          Arguments
        </label>
        <span className="font-mono text-[9px] uppercase tracking-[0.08em] text-faint">
          JSON object
        </span>
      </div>
      <textarea
        ref={fieldRef}
        id={fieldId}
        value={text}
        onChange={(event) => onChange(event.target.value)}
        disabled={disabled}
        spellCheck={false}
        autoComplete="off"
        aria-invalid={error ? true : undefined}
        aria-describedby={error ? errorId : summaryId}
        className={`h-[150px] w-full resize-y rounded-md border bg-bg p-2.5 font-mono text-[10.5px] leading-5 text-ink placeholder:text-faint focus:outline-none focus:ring-1 disabled:cursor-not-allowed disabled:opacity-60 ${
          error
            ? 'border-err/50 focus:border-err/60 focus:ring-err/20'
            : 'border-line focus:border-accent/50 focus:ring-accent/30'
        }`}
      />
      {error ? (
        <p id={errorId} role="alert" className="mt-1 text-[10px] leading-4 text-err">
          {error.message}
        </p>
      ) : (
        <p
          id={summaryId}
          title={summary}
          className="mt-1 truncate font-mono text-[9px] leading-4 text-faint"
        >
          {summary}
        </p>
      )}
    </div>
  );
}
