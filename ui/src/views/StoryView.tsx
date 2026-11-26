import React from 'react';
import { indexFunctions } from '../lib/graph';
import HeaderCard from './HeaderCard';
import { InputsSection, OutputsSection, TriggeredBySection } from './Panels';
import { StepList } from './StepList';
import type { CanvasViewProps } from './props';

/** Story canvas: header, real callers, inputs, steps, returns and declared errors. */
export default function StoryView({
  graph,
  fn,
  onCaller,
  onCallee,
  onInvoke,
}: CanvasViewProps) {
  if (!fn) return null;
  const index = indexFunctions(graph);

  return (
    <div className="mx-auto w-full max-w-[1020px] px-5 py-5 sm:px-7">
      <HeaderCard fn={fn} onInvoke={onInvoke} />
      <div className="mt-4 space-y-3">
        <TriggeredBySection fn={fn} index={index} onCaller={onCaller} />
        <InputsSection fn={fn} />
        <StepList fn={fn} index={index} onCallee={onCallee} />
        <OutputsSection fn={fn} />
      </div>
    </div>
  );
}
