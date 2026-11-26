// Regenerates data.js from the real self-scan manifest. Run from any directory:
//   node wireframe/fnscope-workspace/generate-data.js
// Requires .fnscope/fn-graph.json at the repository root.
const fs = require('node:fs');
const path = require('node:path');

const ROOT = path.resolve(__dirname, '../..');
const OUT = path.join(__dirname, 'data.js');
const manifest = JSON.parse(fs.readFileSync(path.join(ROOT, '.fnscope/fn-graph.json'), 'utf8'));

const LANG_BY_EXT = {
  '.rs': 'Rust', '.ts': 'TypeScript', '.tsx': 'TSX', '.js': 'JavaScript', '.jsx': 'JSX',
  '.py': 'Python', '.go': 'Go', '.java': 'Java', '.c': 'C', '.h': 'C', '.cpp': 'C++',
  '.cc': 'C++', '.hpp': 'C++', '.kt': 'Kotlin', '.cs': 'C#', '.rb': 'Ruby', '.php': 'PHP',
  '.swift': 'Swift',
};

const extOf = (p) => {
  const i = p.lastIndexOf('.');
  return i === -1 ? '' : p.slice(i);
};

const CURATED = new Set([
  'scan-with-registry', 'scan', 'main', 'router', 'write-manifest', 'read-manifest',
  'serve', 'invoke', 'analyze-crates-fnscope-parser-src', 'build-glob-set', 'collect-files',
  'evaluate-grammar-role', 'grade-sentence-exercise', 'tokenize', 'slugify', 'clamp',
  'dispatch-webhook-event', 'infer-syntactic-job', 'formatter-format-tests-fixtures-sample-app-src-utils',
]);

// Curated step stories: same function, narrative grounded in the real source.
const STORY_OVERRIDES = {
  'scan-with-registry': [
    {
      id: 'step-1', kind: 'transform', callsFnId: 'build-glob-set',
      title: 'Compile include and exclude glob sets',
      code: 'let includes = build_glob_set(&options.includes, "include")?;\nlet excludes = build_glob_set(&options.excludes, "exclude")?;',
      inputs: ['options.includes', 'options.excludes'], output: 'includes, excludes',
      detail: 'Glob patterns become matchers once, before any file is touched.',
    },
    {
      id: 'step-2', kind: 'call', callsFnId: 'collect-files',
      title: 'Walk the target directory and collect matching files',
      code: 'collect_files(&options.target_dir, &options.target_dir, &includes, &excludes, &mut paths, &mut warnings);',
      inputs: ['target_dir', 'includes', 'excludes'], output: 'paths: BTreeSet<String>',
      detail: 'Sorted path order makes every later id and type decision deterministic.',
    },
    {
      id: 'step-3', kind: 'transform', callsFnId: null,
      title: 'Read each file and resolve its language adapter',
      code: 'let Some(adapter) = registry.for_extension(extension) else { continue };',
      inputs: ['paths', 'registry'], output: 'scanned: Vec<ScannedFile>',
      detail: 'One extension lookup per file; no per-language branch in the graph builder.',
    },
    {
      id: 'step-4', kind: 'transform', callsFnId: null,
      title: 'Assign stable ids and index bare + qualified names',
      code: 'let base = make_id(&file.relative, &found.qualified_name);\nname_to_id.insert(found.name.clone(), id.clone());',
      inputs: ['scanned'], output: 'name_to_id: BTreeMap<String, String>',
      detail: 'Collisions get a parent-path suffix, so ids stay unique and stable.',
    },
    {
      id: 'step-5', kind: 'call', callsFnId: 'analyze-crates-fnscope-parser-src',
      title: 'Analyze each body through its adapter',
      code: 'let mut body = adapter.analyze(&found, &file.source, &known_names);',
      inputs: ['found', 'file.source', 'known_names'], output: 'BodyAnalysis',
      detail: 'Callee names resolve through the complete project index, not file order.',
    },
    {
      id: 'step-6', kind: 'return', callsFnId: null,
      title: 'Build reverse caller edges and assemble the graph',
      code: 'reverse.entry(callee).or_default().push(caller);\nOk((graph, report))',
      inputs: ['analyzed', 'reverse', 'outgoing'], output: 'Result<(FnGraph, ScanReport)>',
      detail: 'Every edge is written in both directions before the manifest is returned.',
    },
  ],
  invoke: [
    {
      id: 'step-1', kind: 'call', callsFnId: null,
      title: 'Start the wall-clock timer',
      code: 'let started = Instant::now();',
      inputs: [], output: 'started: Instant',
      detail: 'Duration is measured across manifest load, simulation, and the optional Node run.',
    },
    {
      id: 'step-2', kind: 'call', callsFnId: 'load-graph',
      title: 'Load and validate the manifest for this request',
      code: 'let graph = match load_graph(&state.manifest_path).await {\n    Ok(graph) => graph,\n    Err(reason) => return error_response(StatusCode::NOT_FOUND, reason),\n};',
      inputs: ['state.manifest_path'], output: 'graph: FnGraph',
      detail: 'A missing or invalid manifest returns a readable error instead of panicking.',
    },
    {
      id: 'step-3', kind: 'call', callsFnId: 'simulate',
      title: 'Build the deterministic simulated trace',
      code: 'let simulated = simulate(&graph, &request);',
      inputs: ['graph', 'request'], output: 'simulated: InvokeResponse',
      detail: 'Every step gets a trace entry, even when no runtime executes the function.',
    },
    {
      id: 'step-4', kind: 'branch', callsFnId: null,
      title: 'Choose the runtime from the configured invoke mode',
      code: 'let mut response = match state.invoke_mode {\n    InvokeMode::Simulate => simulated.clone(),\n    InvokeMode::Node => node_invoke(...).unwrap_or_else(|_| simulated.clone()),\n};',
      inputs: ['state.invoke_mode', 'simulated'], output: 'response: InvokeResponse',
      detail: 'Node mode is best-effort: any runner failure falls back to the simulated response.',
    },
    {
      id: 'step-5', kind: 'branch', callsFnId: null,
      title: 'Reuse the deterministic trace when the runner returns none',
      code: 'if response.trace.is_empty() {\n    response.trace = simulated.trace.clone();\n}',
      inputs: ['response.trace', 'simulated.trace'], output: 'response.trace',
      detail: "The UI always has per-step data, even when Node's runner cannot report it.",
    },
    {
      id: 'step-6', kind: 'transform', callsFnId: null,
      title: 'Record elapsed wall-clock duration',
      code: 'response.duration_ms = started.elapsed().as_secs_f64() * 1000.0;',
      inputs: ['started'], output: 'response.durationMs',
      detail: 'The number the sandbox displays is measured, not estimated.',
    },
    {
      id: 'step-7', kind: 'return', callsFnId: null,
      title: 'Serialize the response as JSON',
      code: 'Json(response)',
      inputs: ['response'], output: 'Json<InvokeResponse>',
      detail: 'Axum handles the status code and content type.',
    },
  ],
  router: [
    {
      id: 'step-1', kind: 'transform', callsFnId: null,
      title: 'Capture manifest path and invoke mode in shared state',
      code: 'let state = Arc::new(AppState { manifest_path: config.manifest_path, invoke_mode: config.invoke_mode });',
      inputs: ['config: ServerConfig'], output: 'state: Arc<AppState>',
      detail: 'State is cheap to clone, so every route gets its own handle.',
    },
    {
      id: 'step-2', kind: 'call', callsFnId: null,
      title: 'Register the six routes and attach state',
      code: 'Router::new()\n  .route("/", get(index))\n  .route("/api/fn-graph.json", get(fn_graph))\n  .route("/api/invoke", post(invoke))\n  .with_state(state)',
      inputs: ['state'], output: 'Router',
      detail: 'The route table is returned without binding a socket, so tests can drive it directly.',
    },
    {
      id: 'step-3', kind: 'return', callsFnId: null,
      title: 'Return the testable router',
      code: 'Router::new()...with_state(state)',
      inputs: ['routes'], output: 'Router',
      detail: 'No listener is opened here; serve() owns the socket.',
    },
  ],
};

function trimStep(step, curated) {
  const out = {
    id: step.id,
    kind: step.kind,
    callsFnId: step.callsFnId,
    title: step.title,
    inputs: step.inputs,
    output: step.output,
  };
  if (curated) {
    out.code = (step.code || '').slice(0, 900);
    out.detail = step.detail || '';
  }
  return out;
}

function trimFn(fn) {
  const curated = CURATED.has(fn.id);
  const story = STORY_OVERRIDES[fn.id];
  const steps = story || fn.steps.slice(0, 14).map((s) => trimStep(s, curated));
  return {
    id: fn.id,
    name: fn.name,
    module: fn.module,
    filePath: fn.filePath,
    line: fn.line,
    purity: fn.purity,
    description: fn.description,
    parameters: fn.parameters.map((p) => ({
      name: p.name, type: p.type, required: p.required, defaultValue: p.defaultValue,
    })),
    steps,
    stepCount: story ? story.length : fn.steps.length,
    returns: fn.returns,
    errors: fn.errors,
    calledBy: fn.calledBy.map((c) => ({ fnId: c.fnId, name: c.name, module: c.module, callSite: c.callSite })),
    tags: fn.tags,
    sourceCode: curated ? fn.sourceCode.slice(0, 3200) : '',
  };
}

function buildProject(id, label, note, predicate, defaultId, rootDir) {
  const fns = manifest.functions.filter(predicate);
  const langCounts = {};
  const files = new Set();
  for (const fn of fns) {
    files.add(fn.filePath);
    const lang = LANG_BY_EXT[extOf(fn.filePath)] || extOf(fn.filePath).replace('.', '').toUpperCase() || 'Unknown';
    langCounts[lang] = (langCounts[lang] || 0) + 1;
  }
  const languages = Object.entries(langCounts).sort((a, b) => b[1] - a[1]).map(([name, count]) => ({ name, count }));
  return {
    id,
    label,
    note,
    rootDir,
    generatedAt: manifest.generatedAt,
    functionCount: fns.length,
    fileCount: files.size,
    languages,
    defaultId: fns.some((f) => f.id === defaultId) ? defaultId : fns[0] && fns[0].id,
    functions: fns.map(trimFn),
  };
}

const projects = [
  buildProject(
    'fnscope-crates',
    'fnscope-crates',
    'crates/ only — core implementation',
    (f) => f.filePath.startsWith('crates/'),
    'invoke',
    'crates',
  ),
  buildProject(
    'fnscope',
    'fnscope',
    'Full tree — includes test fixtures',
    () => true,
    'invoke',
    '.',
  ),
  buildProject(
    'sample-nlp-app',
    'sample-nlp-app',
    'tests/fixtures/sample_app — isolated fixture',
    (f) => f.filePath.startsWith('tests/fixtures/sample_app'),
    'evaluate-grammar-role',
    'tests/fixtures/sample_app',
  ),
];

const payload = {
  generatedFrom: '.fnscope/fn-graph.json',
  generatedAt: manifest.generatedAt,
  note: 'Prototype data: real functions from a fnscope self-scan, trimmed for the wireframe. Step narratives for invoke, scan_with_registry and router are curated from their source.',
  projects,
};

fs.mkdirSync(path.dirname(OUT), { recursive: true });
fs.writeFileSync(OUT, 'window.FNSCOPE_DATA = ' + JSON.stringify(payload) + ';\n');
const size = fs.statSync(OUT).size;
console.log('wrote', OUT, (size / 1024).toFixed(1) + ' KB');
for (const p of projects) {
  console.log(p.id, p.functionCount, 'fns', p.fileCount, 'files', p.languages.map((l) => l.name + ':' + l.count).join(' '));
}
