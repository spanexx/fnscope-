import React from 'react';

export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: 'primary' | 'secondary' | 'ghost';
  size?: 'sm' | 'md';
  block?: boolean;
}

const VARIANTS = {
  primary: 'border-accent bg-accent text-accent-ink hover:bg-accent/90',
  secondary: 'border-line bg-panel text-ink hover:border-line-strong hover:bg-raised',
  ghost: 'border-transparent text-muted hover:bg-raised hover:text-ink',
};

const SIZES = {
  sm: 'h-8 px-2.5 text-[11px]',
  md: 'h-10 px-3 text-[12px]',
};

/** Shared button; callers own the action, the primitive owns focus and contrast. */
export default function Button({
  variant = 'secondary',
  size = 'sm',
  block = false,
  className = '',
  type = 'button',
  ...rest
}: ButtonProps) {
  return (
    <button
      type={type}
      className={`inline-flex items-center justify-center gap-1.5 rounded-md border font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/50 disabled:cursor-not-allowed disabled:opacity-50 ${VARIANTS[variant]} ${SIZES[size]} ${block ? 'w-full' : ''} ${className}`}
      {...rest}
    />
  );
}
