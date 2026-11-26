// Classifier maps a matched token to a plain-English grammatical job.
import type { Token, SyntacticJob } from '../types.ts';

// Suffixes that reliably mark an action word in this small fixture.
const VERB_SUFFIX = /(ed|ing|s)$/;
// Function words that connect a noun phrase to the rest of the clause.
const PREPOSITIONS = new Set(['over', 'under', 'in', 'on', 'with', 'to', 'from', 'by']);
// Descriptive modifiers that colour the noun after them.
const ADJECTIVES = new Set(['quick', 'lazy', 'red', 'small', 'bright', 'loud']);

export function inferSyntacticJob(match: Token, tokens: Token[]): SyntacticJob {
  // 1. Read the normalized spelling of the matched token.
  const lower = match.normalized;

  // 2. Locate the first verb-like token to split doers from receivers.
  const verbIndex = tokens.findIndex((token) => VERB_SUFFIX.test(token.normalized));

  // 3. An inflected word after the opening token drives the action.
  if (VERB_SUFFIX.test(lower) && match.index > 0) {
    return {
      id: 'engine',
      label: 'Engine (verb)',
      confidence: 0.94,
      intuition: `"${match.raw}" drives the action in this clause.`
    };
  }

  // 4. Relational words act as bridges between phrases.
  if (PREPOSITIONS.has(lower)) {
    return {
      id: 'bridge',
      label: 'Bridge (preposition)',
      confidence: 0.9,
      intuition: `"${match.raw}" links a noun phrase to the rest of the clause.`
    };
  }

  // 5. Descriptive words colour the noun that follows them.
  if (ADJECTIVES.has(lower)) {
    return {
      id: 'painter',
      label: 'Painter (adjective)',
      confidence: 0.89,
      intuition: `"${match.raw}" modifies the noun right after it.`
    };
  }

  // 6. Position relative to the verb separates subjects from objects.
  return verbIndex !== -1 && match.index > verbIndex
    ? {
        id: 'receiver',
        label: 'Receiver (object)',
        confidence: 0.91,
        intuition: `"${match.raw}" receives the action of the verb.`
      }
    : {
        id: 'doer',
        label: 'Doer (subject)',
        confidence: 0.94,
        intuition: `"${match.raw}" anchors the action in this clause.`
      };
}
