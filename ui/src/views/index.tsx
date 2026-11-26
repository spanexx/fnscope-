import React from 'react';
import { CircleSlash } from 'lucide-react';
import EmptyState from '../components/EmptyState';
import MapView from './MapView';
import RawView from './RawView';
import SplitView from './SplitView';
import StoryView from './StoryView';
import type { CanvasViewProps } from './props';

/**
 * Single canvas entry point: the orchestrator passes the loaded graph, the
 * selected record, the active view and the selection/path/invoke callbacks.
 */
export default function CanvasViews(props: CanvasViewProps) {
  if (!props.fn) {
    return (
      <EmptyState
        icon={<CircleSlash className="h-4 w-4" aria-hidden="true" />}
        title="No function selected"
        description="Choose a function in the explorer to see its story, callers, source and raw record."
      />
    );
  }

  switch (props.view) {
    case 'map':
      return <MapView {...props} />;
    case 'split':
      return <SplitView {...props} />;
    case 'raw':
      return <RawView {...props} />;
    case 'story':
    default:
      return <StoryView {...props} />;
  }
}

export { StoryView, MapView, SplitView, RawView };
export type { CanvasViewProps } from './props';
