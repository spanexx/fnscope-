import React, { createContext, useCallback, useContext, useEffect, useMemo, useState } from 'react';
import type { ThemeName } from '../lib/types';

const THEME_STORAGE_KEY = 'fnscope-workspace-theme';

interface ThemeContextValue {
  theme: ThemeName;
  setTheme: (theme: ThemeName) => void;
  toggleTheme: () => void;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

function readTheme(): ThemeName {
  if (typeof document !== 'undefined') {
    const attribute = document.documentElement.getAttribute('data-theme');
    if (attribute === 'paper' || attribute === 'graphite') return attribute;
  }
  try {
    const saved = window.localStorage.getItem(THEME_STORAGE_KEY);
    if (saved === 'paper' || saved === 'graphite') return saved;
  } catch {
    // Storage can be blocked; Paper stays the default.
  }
  return 'paper';
}

/** Owns the Paper/Graphite palette and persists the user's choice. */
export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const [theme, setTheme] = useState<ThemeName>(readTheme);

  useEffect(() => {
    document.documentElement.setAttribute('data-theme', theme);
    try {
      window.localStorage.setItem(THEME_STORAGE_KEY, theme);
    } catch {
      // Persisting is a convenience; the active palette still applies.
    }
  }, [theme]);

  const toggleTheme = useCallback(() => {
    setTheme((current) => (current === 'paper' ? 'graphite' : 'paper'));
  }, []);

  const value = useMemo(() => ({ theme, setTheme, toggleTheme }), [theme, toggleTheme]);
  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useTheme(): ThemeContextValue {
  const context = useContext(ThemeContext);
  if (!context) throw new Error('useTheme must be used inside ThemeProvider');
  return context;
}
