import { useEffect, useRef } from 'react';
import type { RefObject } from 'react';
import { getFocusable } from '../lib/focus';

interface FocusTrapOptions {
  /** Focus the first focusable element when the trap turns on. */
  initialFocus?: boolean;
  /** Return focus to the element that opened the trap when it turns off. */
  restoreFocus?: boolean;
}

/**
 * Trap Tab inside a modal surface and restore focus on close. Escape is handled
 * in the capture phase so the page-level shortcut handler does not double-fire.
 */
export function useFocusTrap(
  containerRef: RefObject<HTMLElement>,
  active: boolean,
  onEscape?: () => void,
  options: FocusTrapOptions = {}
): void {
  const { initialFocus = true, restoreFocus = true } = options;
  const escapeRef = useRef(onEscape);
  escapeRef.current = onEscape;

  useEffect(() => {
    if (!active) return;
    const node = containerRef.current;
    if (!node) return;

    const previous =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;

    if (initialFocus) {
      const first = getFocusable(node)[0];
      if (first) first.focus();
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        event.stopPropagation();
        escapeRef.current?.();
        return;
      }
      if (event.key !== 'Tab') return;

      const items = getFocusable(node);
      if (items.length === 0) {
        event.preventDefault();
        return;
      }
      const index = items.indexOf(document.activeElement as HTMLElement);
      const step = event.shiftKey ? -1 : 1;
      const nextIndex =
        index === -1 ? (event.shiftKey ? items.length - 1 : 0) : (index + step + items.length) % items.length;
      event.preventDefault();
      items[nextIndex].focus();
    };

    document.addEventListener('keydown', handleKeyDown, true);
    return () => {
      document.removeEventListener('keydown', handleKeyDown, true);
      // Only take focus back when it stayed inside the closing surface; an
      // outside click should keep focus where the user put it.
      const focusStayedInside = node.contains(document.activeElement);
      if (
        restoreFocus &&
        previous &&
        document.contains(previous) &&
        (focusStayedInside || document.activeElement === document.body)
      ) {
        previous.focus();
      }
    };
  }, [active, containerRef, initialFocus, restoreFocus]);
}
