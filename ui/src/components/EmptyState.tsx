import React from 'react';

interface EmptyStateProps {
  icon?: React.ReactNode;
  title: string;
  description?: React.ReactNode;
  action?: React.ReactNode;
  compact?: boolean;
  role?: 'status' | 'alert';
}

/** Shared empty/error surface with an explicit title and next action. */
export default function EmptyState({
  icon,
  title,
  description,
  action,
  compact = false,
  role,
}: EmptyStateProps) {
  return (
    <div
      role={role}
      className={`flex flex-col items-center justify-center gap-2 text-center ${compact ? 'px-4 py-6' : 'px-6 py-12'}`}
    >
      {icon ? (
        <div className="grid h-9 w-9 place-items-center rounded-full border border-line bg-raised text-muted">
          {icon}
        </div>
      ) : null}
      <h2 className="text-[13px] font-medium text-ink">{title}</h2>
      {description ? (
        <div className="max-w-[36ch] text-[11.5px] leading-5 text-muted">{description}</div>
      ) : null}
      {action ? <div className="mt-1">{action}</div> : null}
    </div>
  );
}
