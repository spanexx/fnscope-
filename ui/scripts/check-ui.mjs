import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const UI_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const FORBIDDEN = ['fonts.googleapis', 'cdn.tailwindcss', 'unpkg.com'];
const ABSOLUTE = /[A-Za-z]:\\/; // culture-allow: absolute path: detects forbidden drive-letter paths in build output
const WSL_USER = /\/mnt\/c\/Users\//; // culture-allow: absolute path: detects the WSL user mount in build output
const LINUX_HOME = /\/home\//; // culture-allow: absolute path: detects Linux home paths in build output
const SOURCE_EXTS = new Set(['.ts', '.tsx', '.css']);
const MARKERS = ['fnscope:repaired-object-arg', '/api/fn-graph.json'];

function walk(dir, out = []) {
  if (!existsSync(dir)) return out;
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === 'node_modules' || entry.name === 'dist') continue;
    const full = join(dir, entry.name);
    if (entry.isDirectory()) walk(full, out);
    else out.push(full);
  }
  return out;
}

function checkText(label, text) {
  for (const token of FORBIDDEN) {
    if (text.includes(token)) throw new Error(`${label} contains forbidden CDN reference: ${token}`);
  }
  for (const pattern of [ABSOLUTE, WSL_USER, LINUX_HOME]) {
    if (pattern.test(text)) throw new Error(`${label} contains a hardcoded absolute path`);
  }
}

function checkTree(root, sourceOnly) {
  const sourceFiles = walk(join(root, 'src')).filter((file) => SOURCE_EXTS.has(file.slice(file.lastIndexOf('.'))));
  const main = join(root, 'src/main.tsx');
  if (!existsSync(main)) throw new Error('src/main.tsx is missing');
  for (const file of sourceFiles) checkText(file, readFileSync(file, 'utf8'));
  if (sourceOnly) return sourceFiles.length;

  const dist = [
    join(root, 'dist/index.html'),
    join(root, 'dist/theme.js'),
    join(root, 'dist/app.js'),
    join(root, 'dist/app.css'),
  ];
  for (const file of dist) {
    if (!existsSync(file) || statSync(file).size === 0) throw new Error(`missing or empty ${file}`);
  }
  const indexText = readFileSync(dist[0], 'utf8');
  const jsText = readFileSync(dist[2], 'utf8');
  const cssText = readFileSync(dist[3], 'utf8');
  checkText(dist[0], indexText);
  checkText(dist[1], readFileSync(dist[1], 'utf8'));
  checkText(dist[2], jsText);
  checkText(dist[3], cssText);
  if (!indexText.includes('id="root"')) throw new Error('dist/index.html is missing #root');
  if (!indexText.includes('/theme.js')) throw new Error('dist/index.html does not load /theme.js');
  if (statSync(dist[2]).size < 1000) throw new Error('dist/app.js is unexpectedly small');
  if (statSync(dist[3]).size < 100) throw new Error('dist/app.css is unexpectedly small');
  for (const marker of MARKERS) {
    if (!jsText.includes(marker)) throw new Error(`dist/app.js is missing marker: ${marker}`);
  }

  const sourceMax = Math.max(...sourceFiles.map((file) => statSync(file).mtimeMs));
  // One asset can stay byte-identical (Tailwind skips the write), so use the newest
  // asset as the build timestamp rather than the oldest.
  const distNewest = Math.max(...dist.map((file) => statSync(file).mtimeMs));
  if (sourceMax > distNewest) throw new Error('ui/dist is stale; run `bash ui/build.sh`');

  return sourceFiles.length;
}

function selfTest() {
  const sandbox = mkdtempSync(join(tmpdir(), 'fnscope-ui-check-'));
  const make = (name, js) => {
    const root = join(sandbox, name);
    mkdirSync(join(root, 'src'), { recursive: true });
    mkdirSync(join(root, 'dist'), { recursive: true });
    writeFileSync(join(root, 'src/main.tsx'), 'console.log("ok");\n');
    writeFileSync(join(root, 'dist/index.html'), '<div id="root"></div>\n<script src="/theme.js"></script>\n');
    writeFileSync(join(root, 'dist/theme.js'), '// palette bootstrap\n');
    writeFileSync(join(root, 'dist/app.js'), `${'x'.repeat(1200)}\n${js}\n`);
    writeFileSync(join(root, 'dist/app.css'), `${'x'.repeat(120)}\n`);
    return root;
  };

  try {
    const good = make('good', 'fnscope:repaired-object-arg /api/fn-graph.json');
    checkTree(good, false);
    const bad = make('bad', 'https://unpkg.com/lucide');
    let failed = false;
    try { checkTree(bad, false); } catch { failed = true; }
    if (!failed) throw new Error('self-test: forbidden CDN was not rejected');
    console.log('UI_CHECK_SELFTEST_OK');
  } finally {
    rmSync(sandbox, { recursive: true, force: true });
  }
}

const sourceOnly = process.argv.includes('--source');
if (process.argv.includes('--self-test')) {
  selfTest();
} else {
  const count = checkTree(UI_ROOT, sourceOnly);
  console.log(`UI_CHECK_OK (${count} source files${sourceOnly ? ', source only' : ''})`);
}
