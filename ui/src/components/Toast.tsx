import React, { createContext, useCallback, useContext, useEffect, useRef, useState } from 'react';
import { CircleAlert, Info, X } from 'lucide-react';

export type ToastTone = 'info' | 'success' | 'error';

interface ToastItem {
  id: number;
  message: string;
  tone: ToastTone;
}

interface ToastContextValue {
  push: (message: string, tone?: ToastTone) => void;
}

const ToastContext = createContext<ToastContextValue | null>(null);
const TOAST_TIMEOUT_MS = 5000;

const TONE_CLASSES: Record<ToastTone, string> = {
  info: 'border-line-strong bg-raised text-ink',
  success: 'border-ok/40 bg-raised text-ink',
  error: 'border-err/45 bg-raised text-ink',
};

/** Queue user-facing notices in an aria-live region; never fake model output. */
export function ToastProvider({ children }: { children: React.ReactNode }) {
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const timers = useRef(new Map<number, ReturnType<typeof setTimeout>>());
  const sequence = useRef(0);

  const dismiss = useCallback((id: number) => {
    setToasts((current) => current.filter((toast) => toast.id !== id));
    const timer = timers.current.get(id);
    if (timer) {
      clearTimeout(timer);
      timers.current.delete(id);
    }
  }, []);

  const push = useCallback(
    (message: string, tone: ToastTone = 'info') => {
      const id = ++sequence.current;
      setToasts((current) => [...current.slice(-3), { id, message, tone }]);
      const timer = setTimeout(() => dismiss(id), TOAST_TIMEOUT_MS);
      timers.current.set(id, timer);
    },
    [dismiss]
  );

  useEffect(
    () => () => {
      timers.current.forEach((timer) => clearTimeout(timer));
      timers.current.clear();
    },
    []
  );

  return (
    <ToastContext.Provider value={{ push }}>
      {children}
      <div className="ws-toasts" aria-live="polite" aria-atomic="false">
        {toasts.map((toast) => (
          <div
            key={toast.id}
            className={`toast pointer-events-auto flex items-start gap-2 rounded-lg border px-3 py-2.5 shadow-xl ${TONE_CLASSES[toast.tone]}`}
            data-toast-tone={toast.tone}
          >
            <span className="mt-0.5 shrink-0" aria-hidden="true">
              {toast.tone === 'error' ? (
                <CircleAlert className="h-3.5 w-3.5 text-err" />
              ) : (
                <Info className="h-3.5 w-3.5 text-muted" />
              )}
            </span>
            <p className="min-w-0 flex-1 text-[13.5px] leading-5">{toast.message}</p>
            <button
              type="button"
              onClick={() => dismiss(toast.id)}
              aria-label="Dismiss notification"
              className="rounded p-0.5 text-faint transition-colors hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/50"
            >
              <X className="h-3 w-3" />
            </button>
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  );
}

export function useToast(): ToastContextValue {
  const context = useContext(ToastContext);
  if (!context) throw new Error('useToast must be used inside ToastProvider');
  return context;
}
