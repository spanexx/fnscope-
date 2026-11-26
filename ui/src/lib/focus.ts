const FOCUSABLE_SELECTOR = [
  'a[href]',
  'button:not([disabled])',
  'input:not([disabled]):not([type="hidden"])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  'summary',
  '[tabindex]:not([tabindex="-1"])',
].join(',');

/** Real, visible focus targets inside a container, in DOM order. */
export function getFocusable(root: ParentNode | null): HTMLElement[] {
  if (!root) return [];
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter((node) => {
    if (node.hidden || node.getAttribute('aria-hidden') === 'true') return false;
    if (node.hasAttribute('disabled')) return false;
    return node.getClientRects().length > 0;
  });
}

/** Toggle the inert attribute so closed panels cannot receive focus or clicks. */
export function setInert(element: Element | null, inert: boolean): void {
  if (!element) return;
  if (inert) element.setAttribute('inert', '');
  else element.removeAttribute('inert');
}
