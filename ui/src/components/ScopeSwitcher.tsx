import React, { useEffect, useRef, useState } from 'react';
import { Check, ChevronDown, Clock, Shield } from 'lucide-react';
import Badge from './Badge';
import { formatCount } from '../lib/format';
import { formatGeneratedAt } from '../lib/graph';
import { useFocusTrap } from '../state/useFocusTrap';
import type { ScopeDescriptor } from '../lib/types';

interface ScopeSwitcherProps {
  scopes: ScopeDescriptor[];
  activeScope: ScopeDescriptor | null;
  onSelect: (scopeId: string) => void;
  disabled?: boolean;
}

/** Scope popover: every card reports real counts, availability and invoke mode. */
export default function ScopeSwitcher({
  scopes,
  activeScope,
  onSelect,
  disabled = false,
}: ScopeSwitcherProps) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  const close = () => setOpen(false);

  useFocusTrap(menuRef, open, close, { initialFocus: true, restoreFocus: true });

  useEffect(() => {
    if (!open) return;
    const handlePointerDown = (event: PointerEvent) => {
      if (rootRef.current && !rootRef.current.contains(event.target as Node)) close();
    };
    document.addEventListener('pointerdown', handlePointerDown);
    return () => document.removeEventListener('pointerdown', handlePointerDown);
  }, [open]);

  const label = activeScope ? activeScope.label : scopes.length ? 'Select scope' : 'Loading scopes\u2026';
  const stats = activeScope
    ? `${formatCount(activeScope.functionCount)} fns \u00b7 ${activeScope.languages.length} lang${activeScope.languages.length === 1 ? '' : 's'}`
    : '';

  return (
    <div ref={rootRef} className="relative shrink-0">
      <button
        type="button"
        onClick={() => setOpen((current) => !current)}
        disabled={disabled || scopes.length === 0}
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-label={activeScope ? `Scope: ${activeScope.label}. Switch scope` : 'Select scope'}
        className="flex items-center gap-2 rounded-md border border-line bg-panel px-2.5 py-1.5 text-left transition-colors hover:border-line-strong hover:bg-raised focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/50 disabled:cursor-not-allowed disabled:opacity-60"
      >
        <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-accent" aria-hidden="true" />
        <span className="max-w-[120px] truncate text-[14px] font-medium text-ink">{label}</span>
        {stats ? (
          <span className="hidden font-mono text-[12px] text-faint lg:inline">{stats}</span>
        ) : null}
        <ChevronDown className="h-3.5 w-3.5 shrink-0 text-faint" aria-hidden="true" />
      </button>

      {open ? (
        <div
          ref={menuRef}
          role="dialog"
          aria-label="Switch scope"
          className="absolute left-0 top-full z-50 mt-2 w-[390px] max-w-[calc(100vw-24px)] rounded-lg border border-line-strong bg-panel shadow-2xl"
        >
          <div className="p-2">
            <div className="px-2 pb-2 pt-1 font-mono text-[11px] font-semibold uppercase tracking-[0.14em] text-muted">
              Scanned scopes
            </div>
            {scopes.map((scope) => {
              const active = scope.id === activeScope?.id;
              return (
                <button
                  key={scope.id}
                  type="button"
                  disabled={!scope.available}
                  onClick={() => {
                    onSelect(scope.id);
                    setOpen(false);
                  }}
                  title={scope.available ? scope.note || scope.rootDir : scope.error || 'Scope unavailable'}
                  className={`mb-1 w-full rounded-lg border p-2.5 text-left transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/50 ${
                    active
                      ? 'border-accent/40 bg-accent/[0.07]'
                      : 'border-line bg-panel hover:border-line-strong'
                  } ${scope.available ? '' : 'cursor-not-allowed opacity-60'}`}
                >
                  <div className="flex items-start justify-between gap-2">
                    <div className="min-w-0">
                      <div className="flex items-center gap-1.5">
                        <span className="truncate text-[14px] font-medium text-ink">{scope.label}</span>
                        {active ? <Check className="h-3 w-3 shrink-0 text-accent" aria-hidden="true" /> : null}
                      </div>
                      <div className="mt-0.5 truncate text-[12px] text-muted">
                        {scope.note || scope.rootDir || 'No note'}
                      </div>
                    </div>
                    <Badge tone={scope.available ? 'ok' : 'err'}>
                      {scope.available ? 'ready' : 'unavailable'}
                    </Badge>
                  </div>
                  <div className="mt-2 flex flex-wrap items-center gap-1">
                    <span className="font-mono text-[11px] text-faint">
                      {formatCount(scope.functionCount)} fns {'\u00b7'} {formatCount(scope.fileCount)} files
                    </span>
                    {scope.languages.slice(0, 4).map((language) => (
                      <span
                        key={language.name}
                        className="rounded border border-line-strong bg-bg px-1 py-0.5 text-[11px] text-muted"
                      >
                        {language.name}{' '}
                        <span className="text-faint">{formatCount(language.count)}</span>
                      </span>
                    ))}
                    {scope.languages.length > 4 ? (
                      <span className="rounded border border-line-strong px-1 py-0.5 text-[11px] text-faint">
                        +{scope.languages.length - 4}
                      </span>
                    ) : null}
                  </div>
                  <div className="mt-2 flex items-center gap-1 text-[11.5px] text-faint">
                    <Clock className="h-2.5 w-2.5 shrink-0" aria-hidden="true" />
                    <span className="truncate" title={scope.generatedAt || 'generatedAt unknown'}>
                      scanned {scope.generatedAt ? formatGeneratedAt(scope.generatedAt) : 'unknown'}
                      {' \u00b7 '}root {scope.rootDir || 'unknown'}
                    </span>
                  </div>
                  {scope.error ? (
                    <p className="mt-1 break-words font-mono text-[11.5px] text-err">{scope.error}</p>
                  ) : null}
                </button>
              );
            })}
            <div className="mt-2 flex items-start gap-2 rounded-md border border-line bg-bg px-2.5 py-2 text-[12px] leading-4 text-muted">
              <Shield className="mt-0.5 h-3 w-3 shrink-0 text-ok" aria-hidden="true" />
              <span>
                Each scope owns its graph. Switching swaps the sidebar, breadcrumb, canvas and
                sandbox; no cross-repo edges.
              </span>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
