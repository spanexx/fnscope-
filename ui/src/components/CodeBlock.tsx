import React, { useState } from 'react';
import { Check, Copy } from 'lucide-react';

interface CodeBlockProps {
  code: string;
  language?: string;
  copyable?: boolean;
  className?: string;
}

/** Monospace evidence block; copy is opt-in and reports failure silently. */
export default function CodeBlock({ code, language, copyable = false, className = '' }: CodeBlockProps) {
  const [copied, setCopied] = useState(false);

  async function copy() {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      setTimeout(() => setCopied(false), 1600);
    } catch {
      setCopied(false);
    }
  }

  return (
    <div className={`overflow-hidden rounded-md border border-line bg-bg ${className}`}>
      {language || copyable ? (
        <div className="flex items-center justify-between border-b border-line px-2.5 py-1.5">
          <span className="font-mono text-[11px] uppercase tracking-[0.12em] text-faint">
            {language || 'code'}
          </span>
          {copyable ? (
            <button
              type="button"
              onClick={copy}
              className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[12px] text-muted transition-colors hover:bg-raised hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/50"
            >
              {copied ? <Check className="h-3 w-3" aria-hidden="true" /> : <Copy className="h-3 w-3" aria-hidden="true" />}
              {copied ? 'Copied' : 'Copy'}
            </button>
          ) : null}
        </div>
      ) : null}
      <pre className="overflow-x-auto p-3 font-mono text-[13px] leading-5 text-ink">
        <code>{code}</code>
      </pre>
    </div>
  );
}
