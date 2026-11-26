import { test } from 'node:test';
import assert from 'node:assert/strict';
import { build } from 'esbuild';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const outDir = mkdtempSync(join(tmpdir(), 'fnscope-lib-test-'));
const outFile = join(outDir, 'format.mjs');

await build({
  entryPoints: [join(root, 'src/lib/format.ts')],
  bundle: true,
  format: 'esm',
  platform: 'node',
  outfile: outFile,
  logLevel: 'silent',
});

const { shortPath, formatCount } = await import(pathToFileURL(outFile).href);

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

rmSync(outDir, { recursive: true, force: true });
console.log('UI_TEST_OK');
