# fnscope-base — PRD + TRD

Status: SHIPPED (base TS/JS feature; the Rust addendum below is IN PROGRESS until its gates pass)

This is the product and technical requirement set for the first feature of
fnscope. It is derived from `fnscope_architecture_agent_build_spec.md` (the
product spec) and `docs/contract/rust-api.md` (the locked technical contract).
Where they disagree, the contract wins and the gap is recorded in
`docs/drift.md` before the next edit.

## 1. Product goal

fnscope is a local-first developer inspection tool. It pairs raw function
source with a structured Explorer Preview and a 1-Hop Call Neighborhood Map.
It shows what a function actually does as an ordered pipeline of execution
steps, links upstream callers and downstream callees, and offers live
sandbox invocation. It is deterministic first, AI second: structural truth
comes from AST parsing; an LLM is only an optional, hash-cached enrichment
pass. The Rust binary is self-contained and needs no Node at runtime.

## 2. CLI commands

Binary name `fnscope`. Clap derive; the actual top-level parser name is
`FnScopeCli` (the stray `fnscan [ignore]` line in the contract is not a
command and is not implemented).

```
fnscope scan <TARGET_DIR> [--out DIR] [--enrich] [--model NAME] [--include GLOB]... [--exclude GLOB]...
fnscope [TARGET_DIR] | fnscope open [TARGET_DIR] [--port 4310] [--host 127.0.0.1] [--no-watch] [--invoke node|simulate]
```

- `scan` extracts the graph, optionally enriches it, and writes the manifest.
- `open` (or a bare target dir) scans first, then serves the UI on localhost.
- Defaults: out `.fnscope`, port `4310`, host `127.0.0.1`.
- The watcher polls a directory fingerprint every 500 ms and rescans on
  change. `--no-watch` disables it.
- Print concise progress lines. Exit non-zero with a readable error on
  failure.

## 3. Scan artifacts

Both files live under `.fnscope/`. Projects may gitignore or commit them.

- `fn-graph.json` — a `FnGraph`. `tool` is always `"fnscope"`,
  `schemaVersion` always `"1.0.0"`, `generatedAt` RFC3339 UTC, `rootDir` the
  scanned root. Read directly by the UI.
- `enrich-cache.json` — an `EnrichCache` with `version: u32` and `entries`
  keyed by `fn.ast_hash`. Prevents redundant LLM calls.

## 4. Deterministic AST extraction rules (scan pass 1)

Parse `.ts`, `.tsx`, `.js`, `.jsx`. Default includes `**/*.ts`, `**/*.tsx`,
`**/*.js`, `**/*.jsx`; default excludes `**/node_modules/**`, `**/dist/**`,
`**/*.test.ts`, `**/*.spec.ts`, and similar build/test paths.

- **Discovery**: `FunctionDeclaration`; `VariableDeclaration` initialized
  with an arrow or function expression; `MethodDeclaration` in classes, id
  qualified as `ClassName.methodName`. Stable kebab-case id from file +
  qualified name (`make_id`), unique per file and function. Lines are 1-based.
- **Signature**: `parameters` carry `name`, resolved `type`, `required`
  (no question token and no initializer), and the JSDoc `@param` description.
  `returns` carries the resolved type and unpacks object/interface or
  `Promise<T>` properties into `fields: [{ name, type }]`.
- **Purity**: `pure` only if the body does no I/O, mutates no external
  variable, touches no `process.env` / network / DB / filesystem call, and
  calls only pure functions. Otherwise `side-effect`, with inferred
  `permissions` such as `net:outbound`, `fs:read`/`fs:write`, and
  `secrets:read:<KEY>`.
- **Execution steps**: walk the top-level block statements in order and
  classify each as `call`, `branch`, `transform`, or `return`. Each step has
  `id: step-N`, a single-line normalized `code`, `inputs` (identifiers read),
  `output` (assigned variable + type, `throws <CODE> | continues` for a
  guard, or return type), a `title` from the leading `//` comment or a
  deterministic fallback, and a `detail` from a secondary comment or
  deterministic summary. `callsFnId` is the bare callee name when it matches
  a discovered name; core later maps name to id.
- **Errors**: every `ThrowStatement` descendant yields a `code` (class or
  string), optional `status` (e.g. `422`), and `when` from the enclosing `if`
  condition text.
- **Bidirectional edges**: a reverse pass over caller-to-callee edges fills
  `calledBy` on each callee with `fnId` (caller id, or `null` for an external
  entrypoint), `name`, `module` as `<file>:<line>`, and the exact trimmed
  `callSite` line.
- **Hashing**: `normalize_source` strips comments and all whitespace outside
  string/template literals; `ast_hash` is `"sha256:" + hex(sha256(normalized))`.
  Formatting-only changes therefore keep the same hash.

## 5. Schema contract

The exact types, field names, serde renames, and function signatures are in
`docs/contract/rust-api.md` and are not repeated here. Summary:

- `fnscope-model`: `Purity`, `StepKind`, `FnGraph`, `FnRecord`, `CallerRef`,
  `Parameter`, `Step`, `ReturnShape`, `ReturnField`, `ErrorSpec`,
  `EnrichCache`, `EnrichEntry`, `InvokeRequest`, `InvokeResponse`,
  `TraceEntry`, and `FnGraph::validate`.
- All public structs derive `Clone, Debug, Serialize, Deserialize, PartialEq`;
  JSON uses `camelCase` with explicit renames for `type` fields
  (`rename_all = "camelCase"` where noted).
- `fnscope-parser` owns `Lang`, `FnKind`, `DiscoveredFn`, `BodyAnalysis`,
  `discover::discover`, and `body::analyze`.
- `fnscope-core` owns `ScanOptions`, `ScanReport`, `scan`, `write_manifest`,
  `read_manifest`, `normalize_source`, `ast_hash`, and `make_id`.
- `fnscope-enrich` owns `EnrichReport`, `Enricher`, `NoopEnricher`,
  `LlmBackend`, `MockBackend`, `CachedEnricher`, `load_cache`, `write_cache`.
- `fnscope-server` owns `InvokeMode`, `ServerConfig`, `router`, `serve`, and
  the embedded `INDEX_HTML` / `APP_JS` / `APP_CSS` constants.
- `fnscope-cli` owns the clap command surface.

## 6. UI contract

Built from the supplied `fnscope_ui.tsx` into static assets embedded in the
binary. Four view modes:

- `Raw` — `.ts` source or `.json` manifest with one-click Copy.
- `Split` — raw source left, structured Explorer Preview right.
- `Preview` — full-width Explorer Preview tree.
- `1-Hop Map` — three columns: Upstream Callers -> Focused Node -> Downstream
  Calls.

Explorer tree shows metadata, `calledBy`, `parameters`, ordered
`executionSteps` (with call/transform/branch/return badges and clickable
downstream links), `returns`, `throws`, and `invokeSandbox`. Call-path
breadcrumbs support `Jump ↑` and `inspect fn()` with instant back navigation.
The component accepts `graph`, `initialFunctionId`, and `onInvoke` props;
`onInvoke` posts `{ fnId, args }` to `/api/invoke`.

## 7. Server routes

- `GET /` — `INDEX_HTML` as `text/html`.
- `GET /app.js` — `APP_JS` as `text/javascript`.
- `GET /app.css` — `APP_CSS` as `text/css`.
- `GET /api/fn-graph.json` — manifest JSON; `404` if missing, never panic.
- `POST /api/invoke` — JSON `InvokeRequest` -> `InvokeResponse`.
- `GET /api/health` — `{"ok":true,"tool":"fnscope"}`.

`router(config)` is testable without binding a socket. `serve(config)` binds
and runs.

## 8. Cache semantics

Cache key is `fn.ast_hash`. On a hit the entry overwrites `description`,
step titles/details, and parameter defaults with zero API calls. On a miss
only the compact AST skeleton of the new or changed function is sent to the
backend, and the result is stored under that hash. A formatting-only change
keeps the hash and reuses the cache. `NoopEnricher` supplies deterministic
fallbacks with zero API calls. `MockBackend` queues replies and counts calls
for tests. `CachedEnricher::from_env` builds a ureq backend from the API key
environment variables in the spec (`OPENAI_API_KEY`, `GEMINI_API_KEY`,
`ANTHROPIC_API_KEY`).

## 9. Invoke semantics

- `Simulate`: one `TraceEntry` per step with `status: "simulated"` and
  `output: step.output`; `data` is `{ simulated: true, function, inputs,
  expectedReturnType }`.
- `Node`: write a temp runner script, spawn `node`, import the target module
  by absolute file URL, call the named export (class methods as
  `Class.method`), and return JSON `{ ok, data, error, trace }`. On any
  failure, fall back to Simulate and add an `error` field describing why.

## 10. Non-goals

- No whole-repo node graph: navigation is 1-hop only.
- No Node at runtime: Node is build-time only for the UI bundle.
- No writing back to scanned source.
- No changes to the reference codebases.
- No real LLM calls in tests: the mock backend is used.
- No authentication, multi-user support, or deployment concerns.
- No native file watcher crate.
- No languages other than TS/TSX/JS/JSX in this feature.

## 11. Test strategy

- Unit tests live beside the code in `#[cfg(test)]` in each crate; real
  tests only, with edge and regression cases: empty source, formatting-only
  change, missing manifest, failed Node runner.
- `FnGraph::validate` is exercised directly for `tool`, `schemaVersion`,
  unique ids and step ids, and the `sha256:` prefix.
- Parser tests cover discovery and body analysis on small source snippets.
- Core tests cover normalization, hash/id determinism, manifest round-trip,
  and bidirectional `calledBy` edges on the fixture.
- Enrich tests prove hit = 0 calls with field overwrite, miss = 1 call, and
  formatting-only cache reuse.
- Server tests use `router` without binding; end-to-end curl and browser
  verification happen in wave 4 by the orchestrator.
- The orchestrator runs all gates. Smallest check first; full workspace
  suite once at the end; no `cargo` while agents write in parallel.

## 12. Definition of done

The five acceptance items in `.reports/goal-build-fnscope.md`, lines 6-10:
all workspace tests pass; `scan` produces a schema-valid manifest with
deterministic steps, purity, permissions, throws and bidirectional
`calledBy`; enrichment cache behaves as above; `open` serves UI, manifest and
invoke from Windows with `curl.exe`; the UI bundle is built from
`fnscope_ui.tsx` and embedded with no runtime Node.

## Verification evidence

All gates below were run by the orchestrator in WSL Debian after the final
J06/J08 fixes. These are recorded results, not agent claims.

- `cargo test --workspace` -> `CARGO_TEST_STATUS=0`, 72 tests, 0 failures.
  This includes the two real-execution tests
  `node_invocation_runs_a_real_export` and
  `node_invocation_passes_a_scalar_parameter_by_name`.
- `cargo fmt --all -- --check` -> `FMT_STATUS=0`.
- `bash scripts/test-all.sh` -> `ALL_TESTS_OK`, with
  `MANIFEST_OK: 9 functions, 8 call edges` and `SERVER_SMOKE_OK`.
- `bash scripts/enrich-e2e.sh` -> `ENRICH_E2E_OK`: 9 mock calls on the first
  scan, 0 on an unchanged rescan, 0 after a formatting-only change, and a new
  call after a semantic change.
- Browser audit recorded in `.reports/ui-browser.md`: all four views (`Raw`,
  `Split`, `Preview`, `1-Hop Map`), breadcrumb navigation, the 1-Hop Map, and
  real Node invocation (`clamp -> 10`, no post-Run `simulated: true`) against
  the served manifest rather than the built-in demo data.

Known limits are recorded in `VERIFY.md` in this folder.

## Rust source support (addendum)

Added after the base feature shipped, in response to a request to scan a Rust
tree (`crates/`) with the same explorer. Status of this addendum: **IN PROGRESS**
— the base feature remains SHIPPED, but no Rust verification gate has passed
yet. `VERIFY.md` records the pending checks.

- **Requirement**: fnscope must scan `.rs` files and build the same usable
  function graph for them as for TS/JS, so `fnscope scan crates/` reports real
  functions rather than zero files. This extends section 1 (product goal) and
  section 4 (AST extraction); it does not change the UI or server routes.
- **Deterministic extraction**: Rust source is parsed with `tree-sitter-rust`
  through the existing `fnscope-parser` pipeline (`Lang::Rust`, `discover` and
  `analyze`). Functions, signatures, ordered steps, purity/permissions, throws
  and `ast_hash` follow the same deterministic rules as section 4. Structural
  truth stays AST-based; enrichment stays optional and hash-cached.
- **Same schema**: Rust functions produce the same `FnGraph` / `FnRecord`
  types, `tool: "fnscope"`, `schemaVersion: "1.0.0"` and `sha256:` hashes
  defined in section 5 and in `docs/contract/rust-api.md`. There is no new
  manifest file, field or schema version for Rust.
- **Default globs**: `**/*.rs` joins the default includes in section 4 and in
  `ScanOptions`; `**/target/**` joins the default excludes (alongside
  `node_modules`, `dist` and test-file patterns). Users can still override
  these with `--include` / `--exclude`.
- **Non-goal**: executing Rust. Node invocation (section 9) remains a TS/JS
  runner: it imports a TypeScript/JavaScript module through Node and is not a
  Rust executor. Runtime labels stay `universal` / `node20` as carried by the
  schema, including for Rust functions; they describe the recorded target
  environment, not a claim that Node can run Rust. Invoking a Rust function
  yields the deterministic simulated trace. Section 10's "no languages other
  than TS/TSX/JS/JSX" non-goal is therefore superseded for scanning only.
