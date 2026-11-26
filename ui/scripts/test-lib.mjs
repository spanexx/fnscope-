import { test } from 'node:test';
import assert from 'node:assert/strict';
import { build } from 'esbuild';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const outDir = mkdtempSync(join(tmpdir(), 'fnscope-lib-test-'));

async function bundle(entry, name) {
  const outfile = join(outDir, `${name}.mjs`);
  await build({
    entryPoints: [join(root, entry)],
    bundle: true,
    format: 'esm',
    platform: 'node',
    outfile,
    logLevel: 'silent',
  });
  return import(pathToFileURL(outfile).href);
}

const { shortPath, formatCount } = await bundle('src/lib/format.ts', 'format');
const { entryPointCandidates } = await bundle('src/lib/graph.ts', 'graph');
const { ENTRY_POINT_LIMIT, entryPointPage, searchFunctions } = await bundle(
  'src/explorer/entryPoints.ts',
  'entryPoints'
);
const { appendCrumb, backTrail, crumbFor, currentCrumb, prependCrumb, startTrail, truncateTrail } =
  await bundle('src/explorer/trail.ts', 'trail');

/** Minimal real-shaped FnRecord; tests override only the fields under test. */
function fnRecord(overrides) {
  return {
    id: 'id',
    name: 'name',
    module: 'src/a.ts',
    filePath: 'src/a.ts',
    line: 1,
    astHash: `sha256:${'a'.repeat(64)}`,
    version: '1',
    runtime: 'node',
    purity: 'pure',
    timeoutMs: 1000,
    permissions: [],
    description: '',
    tags: [],
    calledBy: [],
    parameters: [],
    steps: [],
    returns: { type: 'void', fields: [] },
    errors: [],
    sourceCode: '',
    ...overrides,
  };
}

function graphOf(functions) {
  return {
    tool: 'fnscope',
    schemaVersion: '1.0.0',
    generatedAt: '2026-11-26T00:00:00Z',
    rootDir: '.',
    functions,
  };
}

test('shortPath keeps the last path segments', () => {
  assert.equal(shortPath('crates/fnscope-server/src/lib.rs'), 'src/lib.rs');
  assert.equal(shortPath('src/main.tsx'), 'src/main.tsx');
  assert.equal(shortPath('main.tsx'), 'main.tsx');
  assert.equal(shortPath('crates\\a\\b\\c.rs', 1), 'c.rs');
});

test('formatCount groups numbers', () => {
  assert.equal(formatCount(981), '981');
  assert.equal(formatCount(1068), '1,068');
  assert.equal(formatCount(0), '0');
});

test('entry points exclude resolved callers and rank by step count', () => {
  const externalCaller = { fnId: null, name: 'outside', module: 'ext', callSite: 'ext:1' };
  const graph = graphOf([
    fnRecord({ id: 'called', name: 'called', calledBy: [{ fnId: 'big', name: 'big', module: 'src/a.ts', callSite: 'src/a.ts:3' }] }),
    fnRecord({ id: 'small', name: 'small', calledBy: [externalCaller], steps: [{ id: 's1' }] }),
    fnRecord({ id: 'big', name: 'big', calledBy: [externalCaller], steps: [{ id: 's1' }, { id: 's2' }] }),
  ]);

  assert.deepEqual(
    entryPointCandidates(graph).map((fn) => fn.id),
    ['big', 'small']
  );

  const firstPage = entryPointPage(graph, false, 1);
  assert.deepEqual(firstPage.items.map((fn) => fn.id), ['big']);
  assert.equal(firstPage.total, 2);
  assert.equal(firstPage.hidden, 1);

  const all = entryPointPage(graph, true, 1);
  assert.equal(all.items.length, 2);
  assert.equal(all.hidden, 1);
  assert.equal(ENTRY_POINT_LIMIT, 24);
});

test('search matches name, id, module and file path case-insensitively', () => {
  const graph = graphOf([
    fnRecord({ id: 'beta', name: 'checkout', module: 'src/billing.ts', filePath: 'src/billing.ts' }),
    fnRecord({ id: 'alpha', name: 'checkoutTotal', module: 'src/orders.ts', filePath: 'src/orders.ts' }),
    fnRecord({ id: 'gamma', name: 'unrelated', module: 'src/BILLING/legacy.ts', filePath: 'src/BILLING/legacy.ts' }),
  ]);

  const byName = searchFunctions(graph, 'CHECKOUT');
  assert.deepEqual(byName.matches.map((fn) => fn.id), ['beta', 'alpha']);
  assert.equal(byName.total, 2);

  const byPath = searchFunctions(graph, 'billing');
  assert.deepEqual(byPath.matches.map((fn) => fn.id), ['beta', 'gamma']);

  const capped = searchFunctions(graph, 'src', 2);
  assert.equal(capped.matches.length, 2);
  assert.equal(capped.total, 3);

  assert.deepEqual(searchFunctions(graph, '   '), { matches: [], total: 0 });
});

test('call-path trail helpers prepend, append, truncate and stop at the root', () => {
  const a = { id: 'a', label: 'a()' };
  const b = { id: 'b', label: 'b()' };
  const c = { id: 'c', label: 'c()' };

  assert.deepEqual(startTrail(crumbFor({ id: 'a', name: 'a' })), [a]);
  assert.deepEqual(prependCrumb([b, c], a), [a, b, c]);
  assert.deepEqual(appendCrumb([a], b), [a, b]);

  assert.deepEqual(truncateTrail([a, b, c], 1), [a, b]);
  assert.deepEqual(truncateTrail([a, b, c], 9), [a, b, c]);
  assert.deepEqual(truncateTrail([a], -1), [a]);

  assert.deepEqual(backTrail([a, b, c]), [a, b]);
  assert.deepEqual(backTrail([a]), [a]);

  assert.deepEqual(currentCrumb([a, b]), b);
  assert.equal(currentCrumb([]), null);
});

rmSync(outDir, { recursive: true, force: true });
console.log('UI_TEST_OK');
