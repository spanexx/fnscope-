// Fake test file. The default exclude glob (**/*.test.ts) must keep it out of the graph.
// If this function shows up in a scan, the default excludes are broken.

export function testEvaluateGrammarRole(): boolean {
  const label = 'placeholder assertion';
  return label.length > 0;
}
