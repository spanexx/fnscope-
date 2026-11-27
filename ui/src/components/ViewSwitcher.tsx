import React, { useEffect, useRef, useState } from 'react';
import { BookOpen, Braces, Columns2, Share2 } from 'lucide-react';
import IconButton from './IconButton';
import { VIEW_IDS } from '../lib/types';
import type { ViewId } from '../lib/types';

const OPTIONS: { id: ViewId; label: string; Icon: typeof BookOpen }[] = [
  { id: 'story', label: 'Story', Icon: BookOpen },
  { id: 'map', label: '1-Hop', Icon: Share2 },
  { id: 'split', label: 'Split', Icon: Columns2 },
  { id: 'raw', label: 'Raw', Icon: Braces },
];

interface ViewSwitcherProps {
  view: ViewId;
  onChange: (view: ViewId) => void;
}

/** Segmented view control; digit shortcuts 1-4 mirror the same order. */
export default function ViewSwitcher({ view, onChange }: ViewSwitcherProps) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const active = OPTIONS.find((option) => option.id === view) ?? OPTIONS[0];

  useEffect(() => {
    if (!open) return;
    const closeOnPointer = (event: PointerEvent) => {
      if (rootRef.current && !rootRef.current.contains(event.target as Node)) setOpen(false);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      // The menu is the top layer; Escape must not also close a docked drawer.
      event.stopPropagation();
      setOpen(false);
    };
    document.addEventListener('pointerdown', closeOnPointer);
    document.addEventListener('keydown', closeOnEscape);
    return () => {
      document.removeEventListener('pointerdown', closeOnPointer);
      document.removeEventListener('keydown', closeOnEscape);
    };
  }, [open]);

  return (
    <>
      <div ref={rootRef} className="relative shrink-0 sm:hidden">
        <IconButton
          label={`Canvas view: ${active.label}. Switch view`}
          aria-haspopup="menu"
          aria-expanded={open}
          onClick={() => setOpen((current) => !current)}
        >
          <active.Icon className="h-4 w-4" aria-hidden="true" />
        </IconButton>
        {open ? (
          <div
            role="menu"
            aria-label="Canvas view"
            className="absolute right-0 top-full z-50 mt-2 w-40 rounded-lg border border-line-strong bg-panel p-1 shadow-2xl"
          >
            {OPTIONS.map(({ id, label, Icon }, index) => {
              const selected = view === id;
              return (
                <button
                  key={id}
                  type="button"
                  role="menuitemradio"
                  aria-checked={selected}
                  onClick={() => {
                    onChange(id);
                    setOpen(false);
                  }}
                  className={`flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-[11px] transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/50 ${
                    selected ? 'bg-active text-ink' : 'text-muted hover:bg-raised hover:text-ink'
                  }`}
                >
                  <Icon className="h-3.5 w-3.5" aria-hidden="true" />
                  <span className="flex-1">{label}</span>
                  <kbd className="font-mono text-[9px] text-faint">{index + 1}</kbd>
                </button>
              );
            })}
          </div>
        ) : null}
      </div>

      <div
        role="group"
        aria-label="Canvas view"
        className="hidden shrink-0 items-center gap-0.5 rounded-md border border-line bg-panel p-0.5 sm:flex"
      >
        {OPTIONS.map(({ id, label, Icon }, index) => {
          const selected = view === id;
          return (
            <button
              key={id}
              type="button"
              onClick={() => onChange(id)}
              aria-pressed={selected}
              aria-label={`${label} view`}
              title={`${label} view (${index + 1})`}
              className={`flex items-center gap-1.5 rounded px-2 py-1 text-[11px] transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/50 ${
                selected ? 'bg-active text-ink shadow-sm' : 'text-muted hover:text-ink'
              }`}
            >
              <Icon className="h-3 w-3" aria-hidden="true" />
              <span className="hidden lg:inline">{label}</span>
            </button>
          );
        })}
        <span className="sr-only">Views 1 through {VIEW_IDS.length}</span>
      </div>
    </>
  );
}
