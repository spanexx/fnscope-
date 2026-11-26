import React from 'react';

export interface IconButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  /** Required accessible name; icons alone are not labels. */
  label: string;
  active?: boolean;
  size?: 'sm' | 'md';
}

const SIZES = {
  sm: 'h-7 w-7',
  md: 'h-8 w-8',
};

/** Square icon control with a visible focus ring and a required aria-label. */
export default function IconButton({
  label,
  active = false,
  size = 'md',
  className = '',
  type = 'button',
  title,
  ...rest
}: IconButtonProps) {
  const tone = active
    ? 'border-accent/30 bg-accent/15 text-accent'
    : 'border-transparent text-muted hover:border-line hover:bg-raised hover:text-ink';
  return (
    <button
      type={type}
      aria-label={label}
      title={title ?? label}
      className={`grid shrink-0 place-items-center rounded-md border transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/50 disabled:cursor-not-allowed disabled:opacity-40 ${SIZES[size]} ${tone} ${className}`}
      {...rest}
    />
  );
}
