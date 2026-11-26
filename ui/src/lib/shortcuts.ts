import type { ViewId } from './types';

/** True on Apple platforms so hints show Cmd instead of Ctrl. */
export function isMacPlatform(): boolean {
  if (typeof navigator === 'undefined') return false;
  const platform = navigator.platform || '';
  const userAgent = navigator.userAgent || '';
  return /Mac|iPhone|iPad|iPod/.test(platform) || /Mac OS X/.test(userAgent);
}

/** Never fire single-key shortcuts while the user is editing text. */
export function isTypingTarget(target: EventTarget | null): boolean {
  if (!target || !(target instanceof HTMLElement)) return false;
  const tag = target.tagName;
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || target.isContentEditable;
}

/** Hint for the search shortcut shown in the sidebar. */
export function searchShortcutHint(): string {
  return isMacPlatform() ? '\u2318K' : 'Ctrl+K';
}

/** Hint for the invoke shortcut shown in the drawer. */
export function runShortcutHint(): string {
  return isMacPlatform() ? '\u2318\u21B5' : 'Ctrl+Enter';
}

/** Map a plain digit to a canvas view; other keys return null. */
export function viewForShortcut(key: string): ViewId | null {
  switch (key) {
    case '1':
      return 'story';
    case '2':
      return 'map';
    case '3':
      return 'split';
    case '4':
      return 'raw';
    default:
      return null;
  }
}
