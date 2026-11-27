import React from 'react';
import { ChevronRight } from 'lucide-react';

export interface Crumb {
  id: string;
  label: string;
}

interface BreadcrumbBarProps {
  trail: Crumb[];
  onSelect?: (index: number) => void;
}

/** Call-path breadcrumb slot; an empty trail states that plainly. */
export default function BreadcrumbBar({ trail, onSelect }: BreadcrumbBarProps) {
  return (
    <nav
      aria-label="Call path"
      className="no-scrollbar flex min-w-0 items-center gap-0.5 overflow-x-auto"
    >
      {trail.length === 0 ? (
        <span className="whitespace-nowrap px-1.5 py-1 font-mono text-[12.5px] text-faint">
          No function selected
        </span>
      ) : (
        trail.map((crumb, index) => {
          const current = index === trail.length - 1;
          return (
            <React.Fragment key={`${crumb.id}-${index}`}>
              {index > 0 ? (
                <ChevronRight className="h-3 w-3 shrink-0 text-faint" aria-hidden="true" />
              ) : null}
              <button
                type="button"
                onClick={() => onSelect?.(index)}
                aria-current={current ? 'page' : undefined}
                title={crumb.label}
                className={`max-w-[150px] truncate rounded border px-1.5 py-1 font-mono text-[13px] transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/50 ${
                  current
                    ? 'border-accent/25 bg-accent/10 text-accent'
                    : 'border-transparent text-muted hover:bg-wash hover:text-ink'
                }`}
              >
                {crumb.label}
              </button>
            </React.Fragment>
          );
        })
      )}
    </nav>
  );
}
