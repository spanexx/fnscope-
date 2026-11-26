import React from 'react';

type BadgeTone = 'neutral' | 'accent' | 'info' | 'ok' | 'warn' | 'err';

const TONE_CLASSES: Record<BadgeTone, string> = {
  neutral: 'border-line bg-raised text-muted',
  accent: 'border-accent/30 bg-accent/10 text-accent',
  info: 'border-info/30 bg-info/10 text-info',
  ok: 'border-ok/30 bg-ok/10 text-ok',
  warn: 'border-warn/30 bg-warn/10 text-warn',
  err: 'border-err/35 bg-err/10 text-err',
};

interface BadgeProps {
  tone?: BadgeTone;
  children: React.ReactNode;
  title?: string;
  className?: string;
}

/** Small metadata chip; language stays neutral so colour signals state only. */
export default function Badge({ tone = 'neutral', children, title, className = '' }: BadgeProps) {
  return (
    <span
      title={title}
      className={`inline-flex items-center gap-1 rounded border px-1.5 py-0.5 font-mono text-[9px] uppercase tracking-[0.08em] ${TONE_CLASSES[tone]} ${className}`}
    >
      {children}
    </span>
  );
}
