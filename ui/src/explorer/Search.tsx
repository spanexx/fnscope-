import React from 'react';
import { Search as SearchIcon, X } from 'lucide-react';
import { searchShortcutHint } from '../lib/shortcuts';

interface SearchProps {
  value: string;
  onChange: (value: string) => void;
  inputRef: React.RefObject<HTMLInputElement>;
  disabled?: boolean;
}

/** Sidebar filter: one input, a clear affordance and the keyboard hint. */
export default function Search({ value, onChange, inputRef, disabled = false }: SearchProps) {
  return (
    <>
      <div className="relative">
        <label htmlFor="explorer-search" className="sr-only">
          Filter functions, modules and ids
        </label>
        <SearchIcon
          className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-faint"
          aria-hidden="true"
        />
        <input
          id="explorer-search"
          ref={inputRef}
          type="search"
          autoComplete="off"
          spellCheck={false}
          value={value}
          onChange={(event) => onChange(event.target.value)}
          disabled={disabled}
          placeholder={'Filter functions, modules, IDs\u2026'}
          className="h-8 w-full rounded-md border border-line bg-panel pl-8 pr-12 text-[11.5px] text-ink placeholder:text-faint focus:border-accent/60 focus:outline-none focus:ring-1 focus:ring-accent/20 disabled:cursor-not-allowed disabled:opacity-60 [&::-webkit-search-cancel-button]:appearance-none"
        />
        {value ? (
          <button
            type="button"
            onClick={() => onChange('')}
            aria-label="Clear search"
            className="absolute right-1.5 top-1/2 -translate-y-1/2 rounded p-1 text-muted transition-colors hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/50 motion-reduce:transition-none"
          >
            <X className="h-3 w-3" aria-hidden="true" />
          </button>
        ) : (
          <kbd className="pointer-events-none absolute right-2 top-1/2 -translate-y-1/2 rounded-sm border border-line bg-raised px-1 py-0.5 font-mono text-[9px] text-faint">
            {searchShortcutHint()}
          </kbd>
        )}
      </div>
      <p className="mt-1.5 font-mono text-[9px] text-faint">/ also focuses search</p>
    </>
  );
}
