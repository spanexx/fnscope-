// Tokenizer isolates punctuation-free word tokens used by the grammar pipeline.
import type { Token } from '../types.ts';

export function tokenize(sentence: string): Token[] {
  // 1. Strip punctuation delimiters and trim the outer whitespace.
  const cleaned = sentence.replace(/[.,!?;:"()]/g, ' ').trim();

  // 2. Return an empty stream when the sentence holds no words.
  if (!cleaned) return [];

  // 3. Split on whitespace and attach a zero-based index to each word.
  return cleaned
    .split(/\s+/)
    .filter(Boolean)
    .map((raw, index) => ({
      index,
      raw,
      normalized: raw.toLowerCase()
    }));
}
