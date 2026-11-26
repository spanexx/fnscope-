// Plain-JavaScript legacy helper kept to prove .js discovery works too.

export function clamp(value, min, max) {
  // Swap inverted bounds so the range is always ordered.
  if (min > max) {
    [min, max] = [max, min];
  }

  // Return the nearest bound when the value falls outside the range.
  return Math.min(Math.max(value, min), max);
}
