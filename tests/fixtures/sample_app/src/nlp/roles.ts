// Roles turns a sentence and target word into a structured grammatical verdict.
import type { Token, RoleEvaluationResult } from '../types.ts';
import { FunctionError } from '../utils/errors.ts';
import { tokenize } from './tokenizer.ts';
import { inferSyntacticJob } from './classifier.ts';

/**
 * Classifies a target word's grammatical job inside a sentence.
 * @param sentence Full sentence providing syntactic context.
 * @param targetWord Word inside the sentence whose job should be identified.
 * @returns Structured role evaluation with optional teaching intuition.
 * @tags nlp, pure
 */
export async function evaluateGrammarRole(input: {
  sentence: string;
  targetWord: string;
  includeTeachingLayer?: boolean;
}): Promise<RoleEvaluationResult> {
  // 1. Tokenize input sentence
  const tokens: Token[] = tokenize(input.sentence);

  // 2. Reject a sentence that produced no word tokens.
  if (tokens.length === 0) {
    throw new FunctionError('EMPTY_SENTENCE', { sentence: input.sentence });
  }

  // 3. Normalize the target and locate its token in the stream.
  const normalizedTarget = input.targetWord.trim().toLowerCase();
  const match = tokens.find((token) => token.normalized === normalizedTarget);

  // 4. Fail fast when the target word is absent from the sentence.
  if (!match) {
    throw new FunctionError('TOKEN_NOT_FOUND', { target: input.targetWord });
  }

  // 5. Infer the syntactic job from the surrounding tokens.
  const role = inferSyntacticJob(match, tokens);

  // 6. Return the role payload with intuition only when requested.
  return {
    roleId: role.id,
    label: role.label,
    confidence: role.confidence,
    ...(input.includeTeachingLayer ? { intuition: role.intuition } : {})
  };
}
