import { readFileSync } from 'node:fs';

function fail(message) {
  throw new Error(message);
}

function checkRecord(fn, index) {
  const label = `functions[${index}]`;
  for (const key of ['id', 'name', 'module', 'filePath', 'line', 'astHash', 'purity', 'parameters', 'steps', 'returns', 'errors', 'sourceCode']) {
    if (!(key in fn)) fail(`${label} is missing ${key}`);
  }
  if (!String(fn.astHash).startsWith('sha256:')) fail(`${label}.astHash is not a sha256 hash`);
  if (!Array.isArray(fn.parameters)) fail(`${label}.parameters is not an array`);
  if (!Array.isArray(fn.steps)) fail(`${label}.steps is not an array`);
  for (const step of fn.steps) {
    for (const key of ['id', 'kind', 'title', 'inputs', 'output']) {
      if (!(key in step)) fail(`${label}.steps[] is missing ${key}`);
    }
  }
}

function checkGraph(graph) {
  for (const key of ['tool', 'schemaVersion', 'generatedAt', 'rootDir', 'functions']) {
    if (!(key in graph)) fail(`manifest is missing ${key}`);
  }
  if (graph.tool !== 'fnscope') fail('manifest.tool is not fnscope');
  if (graph.schemaVersion !== '1.0.0') fail('manifest.schemaVersion is not 1.0.0');
  if (!Array.isArray(graph.functions) || graph.functions.length === 0) fail('manifest has no functions');
  graph.functions.forEach(checkRecord);
  if (!graph.functions.some((fn) => typeof fn.sourceCode === 'string' && fn.sourceCode.length > 0)) {
    fail('manifest has no non-empty sourceCode');
  }
  if (!graph.functions.some((fn) => Array.isArray(fn.steps) && fn.steps.length > 0)) {
    fail('manifest has no populated steps');
  }
}

function selfTest() {
  const good = {
    tool: 'fnscope',
    schemaVersion: '1.0.0',
    generatedAt: '2026-11-26T00:00:00Z',
    rootDir: '.',
    functions: [{
      id: 'a', name: 'a', module: 'm', filePath: 'a.ts', line: 1,
      astHash: `sha256:${'a'.repeat(64)}`, purity: 'pure',
      parameters: [], steps: [{ id: 'step-1', kind: 'return', title: 'Return', inputs: [], output: 'a' }],
      returns: { type: 'string', fields: [] }, errors: [], sourceCode: 'function a() {}',
    }],
  };
  checkGraph(good);
  const bad = { ...good };
  delete bad.rootDir;
  let failed = false;
  try { checkGraph(bad); } catch { failed = true; }
  if (!failed) throw new Error('self-test: missing rootDir was not rejected');
  console.log('UI_CONTRACT_SELFTEST_OK');
}

const arg = process.argv[2];
if (arg === '--self-test') {
  selfTest();
} else if (!arg) {
  console.error('usage: node scripts/check-ui-contract.mjs <fn-graph.json> | --self-test');
  process.exit(2);
} else {
  const graph = JSON.parse(readFileSync(arg, 'utf8'));
  checkGraph(graph);
  console.log(`UI_CONTRACT_OK (${graph.functions.length} functions)`);
}
