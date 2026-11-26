// Grading compares a learner's chosen role with the expected role and logs telemetry.
import type { GradeOutcome } from '../types.ts';
import { evaluateGrammarRole } from '../nlp/roles.ts';
import { dispatchWebhookEvent } from '../events/delivery.ts';

export async function gradeSentenceExercise(input: {
  sentence: string;
  targetWord: string;
  selectedRoleId: string;
}): Promise<GradeOutcome> {
  // 1. Evaluate the ground-truth role for the target word.
  const expected = await evaluateGrammarRole({
    sentence: input.sentence,
    targetWord: input.targetWord,
    includeTeachingLayer: true
  });

  // 2. Compare the learner's selection with the expected role id.
  const correct = expected.roleId === input.selectedRoleId;

  // 3. Emit an exercise.graded webhook receipt.
  const receipt = await dispatchWebhookEvent({
    endpointUrl: 'https://hooks.example.dev/v1/ingest',
    eventType: 'exercise.graded',
    dryRun: true
  });

  // 4. Return the verdict together with delivery metadata.
  return { correct, expectedRole: expected, receipt };
}
