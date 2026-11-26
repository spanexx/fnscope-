import { useEffect, useState } from 'react';

/** Reactive media query with a safe server/first-render fallback. */
export function useMediaQuery(query: string): boolean {
  const [matches, setMatches] = useState(() => {
    if (typeof window === 'undefined' || !window.matchMedia) return false;
    return window.matchMedia(query).matches;
  });

  useEffect(() => {
    if (typeof window === 'undefined' || !window.matchMedia) return;
    const list = window.matchMedia(query);
    const update = () => setMatches(list.matches);
    update();
    if (list.addEventListener) list.addEventListener('change', update);
    else list.addListener(update);
    return () => {
      if (list.removeEventListener) list.removeEventListener('change', update);
      else list.removeListener(update);
    };
  }, [query]);

  return matches;
}
