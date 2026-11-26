import type { Crumb } from '../components/BreadcrumbBar';
import type { FnGraph, FnRecord, ViewId } from '../lib/types';

/**
 * Canvas contract shared by all four views. Selection, call path and drawer
 * state stay with the orchestrator; views only read the manifest and report intent.
 */
export interface CanvasViewProps {
  /** Served manifest, used only to resolve real caller/callee records. */
  graph: FnGraph;
  /** Selected record; null renders the shared no-selection empty state. */
  fn: FnRecord | null;
  /** Which of the four canvases to render. */
  view: ViewId;
  /** Follow an incoming caller (prepend to the call path). */
  onCaller: (fnId: string) => void;
  /** Follow an outgoing callee (append to the call path). */
  onCallee: (fnId: string) => void;
  /** Plain selection, used by compact step inspection. */
  onSelect: (fnId: string) => void;
  /** Open the invoke sandbox for the selected function. */
  onInvoke: () => void;
  /** Optional call path; views read it but never mutate it. */
  trail?: Crumb[];
}
