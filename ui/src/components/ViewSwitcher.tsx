import React from 'react';
import { BookOpen, Braces, Columns2, Share2 } from 'lucide-react';
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
  return (
    <div
      role="group"
      aria-label="Canvas view"
      className="hidden shrink-0 items-center gap-0.5 rounded-md border border-line bg-panel p-0.5 sm:flex"
    >
      {OPTIONS.map(({ id, label, Icon }, index) => {
        const active = view === id;
        return (
          <button
            key={id}
            type="button"
            onClick={() => onChange(id)}
            aria-pressed={active}
            aria-label={`${label} view`}
            title={`${label} view (${index + 1})`}
            className={`flex items-center gap-1.5 rounded px-2 py-1 text-[11px] transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/50 ${
              active ? 'bg-active text-ink shadow-sm' : 'text-muted hover:text-ink'
            }`}
          >
            <Icon className="h-3 w-3" aria-hidden="true" />
            <span className="hidden lg:inline">{label}</span>
          </button>
        );
      })}
      <span className="sr-only">Views 1 through {VIEW_IDS.length}</span>
    </div>
  );
}
