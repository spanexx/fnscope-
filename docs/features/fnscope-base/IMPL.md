# fnscope-base — IMPL (implementation plan)

Status: SHIPPED (base TS/JS feature; Rust addendum J15-J18 pending verification)

This maps the fourteen original delivered jobs to files and to the gate each
job had to pass. The original eleven are listed in `.reports/goal-build-fnscope.md`
(lines 27-48); J12-J14 were added during verification (iterations 9-10). J15-J18
are the Rust addendum jobs dispatched in iteration 15; J18 (this documentation
update) is still in progress.
Jobs ran only on disjoint file sets. The orchestrator owned the workspace
root, the parser type and lib files, the contract, the UI placeholders, and
every gate. An agent wrote files and reported; it did not run cargo or claim
its own work passed.

## Delivered jobs

| Job | File area owned | Delivered |
|-----|-----------------|-----------|
| J01 | `crates/fnscope-model/` | JSON schema types, `validate()`, 10 unit tests |
| J02 | `crates/fnscope-parser/src/discover.rs` | tree-sitter discovery, signatures, project-wide type index helpers |
| J03 | `crates/fnscope-parser/src/body.rs` | steps, throws, purity, permissions, callees |
| J04 | `crates/fnscope-core/` | scan, ids/hashes, bidirectional call graph, manifest I/O, cross-file return fields |
| J05 | `crates/fnscope-enrich/` | AST-hash cache, backend trait, mock/HTTP/noop enrichers |
| J06 | `crates/fnscope-server/` | axum routes, embedded UI, Node/simulate invoke with argv dispatch |
| J07 | `crates/fnscope-cli/` | clap `scan`/`open`, polling watcher, progress and errors |
| J08 | `ui/` | compiled static assets from `fnscope_ui.tsx`, metadata/breadcrumb/arg fixes |
| J09 | `tests/fixtures/sample_app/` | TS/JS fixture project with calls, throws, purity and side effects |
| J10 | `docs/drift.md`, `docs/features/fnscope-base/`, `docs/ethics/` | factory docs: GRILL, PRD-TRD, IMPL, drift, ethics, VERIFY |
| J11 | `scripts/` | bounded smoke harness: `test-all.sh`, `smoke-server.sh`, `check-manifest.mjs` |
| J12 | `scripts/enrich-e2e.sh`, `tests/e2e/` | enrichment end-to-end harness with a local mock LLM |
| J13 | `.reports/ui-browser.md` | real browser audit against the served release binary (evidence only) |
| J14 | `README.md` (root) | user-facing project README |
| J15 | `crates/fnscope-parser/src/discover.rs`, `body.rs` | Rust backend via `tree-sitter-rust` (`Lang::Rust`): discovery, signatures, steps, throws, purity |
| J16 | `crates/fnscope-core/` | `.rs` file discovery: `**/*.rs` default include, `**/target/**` default exclude |
| J17 | `tests/fixtures/rust_app/` | Rust fixture project for real scans |
| J18 | `README.md` (root), `docs/features/fnscope-base/` | user-facing and feature docs for the Rust addendum |

The Rust addendum (J15-J18) is **pending verification**: its jobs write files
only and the orchestrator reviews each one before running any gate. No Rust
gate has passed yet, so the addendum is not marked SHIPPED; the checks that
will be run are listed in `VERIFY.md` with `PENDING` verdicts.

Every fix was reviewed and sent back by the orchestrator rather than
self-approved: compiler errors (J02/J03 borrow and type errors), test
expectation errors (J01 aggregation assertion, J02 dedup assertion), cross-OS
path translation (J06 runner argv and scratch-dir placement), and the UI arg
contract (J08 metadata/breadcrumb/arg repair, J06 parameter-name dispatch).
No agent ran cargo or merged its own work.

## Wave and ownership map

| Job | Wave | Files owned | Deliverable | Gate |
|-----|------|-------------|-------------|------|
| J01 | 1 | `crates/fnscope-model/` | JSON schema types, `validate()`, unit tests | `cargo test -p fnscope-model` |
| J02 | 1 | `crates/fnscope-parser/src/discover.rs` | tree-sitter function discovery + signatures | parser discover tests |
| J03 | 1 | `crates/fnscope-parser/src/body.rs` | steps, throws, purity, permissions, callees | parser body tests |
| J08 | 1 | `ui/` | compiled static assets from `fnscope_ui.tsx` | bundle + embed check |
| J09 | 1 | `tests/fixtures/sample_app/` | realistic TS/JS fixture project | fixture scan check |
| J10 | 1 | `docs/drift.md`, `docs/features/fnscope-base/`, `docs/ethics/` | factory docs (this plan, PRD-TRD, GRILL, drift, ethics) | docs + drift check |
| J11 | 2/3 | `scripts/` | bounded test/smoke harness | detached, bounded, clean run |
| J04 | 2 | `crates/fnscope-core/` | discovery, id/hash, call graph, manifest I/O | `cargo test -p fnscope-core` |
| J05 | 2 | `crates/fnscope-enrich/` | hash cache, backend trait, mock + HTTP, noop | `cargo test -p fnscope-enrich` |
| J06 | 3 | `crates/fnscope-server/` | axum router, embedded UI, `/api/*`, invoke | `cargo test -p fnscope-server` + curl |
| J07 | 3 | `crates/fnscope-cli/` | clap `scan`/`open`, watcher, progress | `cargo test -p fnscope-cli` + scan smoke |
| Wave 4 | 4 | orchestrator only | full gates, review/criticise, end-to-end, drift reconciliation | all acceptance evidence |

## Job detail

### J01 — model
Write `crates/fnscope-model/` with the exact types and serde renames in the
contract, `FnGraph::validate`, and unit tests beside the code. No other
crate's files.
**Gate:** `cargo test -p fnscope-model` passes; tests cover schema round-trip
and rejection of wrong `tool`/`schemaVersion`, duplicate ids, bad step ids,
and a hash without the `sha256:` prefix.

### J02 — discovery
Own `discover.rs` only. Implement `discover::discover(lang, source,
relative_path)` for `Lang::{TypeScript,Tsx,JavaScript,Jsx}`: declarations,
arrows, function expressions, class methods, exact source slice, parameters,
return shape, JSDoc description/tags, async flag, 1-based lines.
**Gate:** parser tests prove all four languages, method qualification as
`ClassName.method`, and exact source slicing. Signatures match the contract.

### J03 — body analysis
Own `body.rs` only. Implement `body::analyze(found, lang, source,
known_names)`: ordered `Step`s with the four `StepKind`s, `ErrorSpec`s,
`Purity`, permissions, callee names in step order, timeout and runtime.
`callsFnId` is the bare name when it matches `known_names`.
**Gate:** parser tests cover guards, throws, side-effect permissions, and
callee ordering. Signatures match the contract.

### J04 — core
Own the whole crate. Implement `ScanOptions`, `ScanReport`, `scan`,
`write_manifest`, `read_manifest`, `normalize_source`, `ast_hash`, `make_id`.
Glob discovery, parse, analyze, build ids, resolve name-to-id, fill reverse
`called_by` (external callers get `fn_id: None`), assign hashes, return an
un-enriched graph with deterministic fallbacks.
**Gate:** `cargo test -p fnscope-core` passes; fixture scan yields a valid
`FnGraph` with deterministic hashes/ids, bidirectional edges, and a
manifest round-trip.

### J05 — enrich
Own the whole crate. Implement `EnrichReport`, `Enricher`, `NoopEnricher`,
`LlmBackend`, `MockBackend`, `CachedEnricher`, `load_cache`, `write_cache`,
and `from_env`. Cache key is `ast_hash`; a hit overwrites description, step
titles/details, and defaults with zero API calls.
**Gate:** `cargo test -p fnscope-enrich` passes; mock tests show hit = 0
calls, miss = 1 call, formatting-only reuse, and cache round-trip.

### J06 — server
Own the whole crate. Implement `InvokeMode`, `ServerConfig`, `router`,
`serve`, and embedded `INDEX_HTML` / `APP_JS` / `APP_CSS`. Six routes per the
contract, 404 on a missing manifest without panic, Simulate trace shape, and
Node invocation with Simulate fallback plus an `error` field.
**Gate:** `cargo test -p fnscope-server` passes with router tests that do not
bind; in wave 4 the orchestrator verifies live routes with `curl.exe` and
then stops the server and frees the port.

### J07 — CLI
Own the whole crate. Implement clap `scan` and `open`/bare target, default out
`.fnscope`, default port `4310`, host `127.0.0.1`, `--no-watch`, `--invoke
node|simulate`, polling fingerprint every 500 ms, concise progress, non-zero
exit with a readable error.
**Gate:** `cargo test -p fnscope-cli` passes; `cargo run -p fnscope -- scan
<fixture> --out <dir>` produces a valid manifest (wave 4 smoke). `open` is
verified separately from tests, never in the same command as a test.

### J08 — UI
Copy the supplied `fnscope_ui.tsx` and bundle it into `ui/dist/index.html`,
`app.js`, `app.css`. Replace the stub files; no runtime Node.
**Gate:** the three dist files are real compiled assets, not stubs; the
server crate embeds them with `include_str!`; the source TSX is unchanged.
Check that the four view modes and the invoke hook exist in the built code.

### J09 — fixture
Create `tests/fixtures/sample_app/` with realistic TS/JS: exported functions,
arrows, class methods, guard throws, cross-file calls, a side-effect
function, and an external entrypoint.
**Gate:** a scan produces a non-trivial graph with expected function count,
steps, errors, purity, permissions, and bidirectional `calledBy` edges.

### J10 — docs (this job)
Write `GRILL.txt`, `PRD-TRD.md`, `IMPL.md`, `docs/drift.md`, and the six
ethics files. Do not edit `AGENTS.md`, the contract, `.reports/`, crates,
`ui/`, `scripts/`, or tests.
**Gate:** all files present; prompts quoted verbatim with no invented user
quote; all answers marked `[agent decision]`; drift log starts with zero
open entries; no drift entry authored to permit this job's own edit.

### J11 — scripts
Build a bounded test/smoke harness under `scripts/`. Dispatched in wave 2/3
so it does not depend on a finished CLI. Multiline logic lives in a script,
not a heredoc across the Windows/WSL boundary.
**Gate:** long runs detached with a log outside the repo, every wait bounded,
servers stopped and ports freed, temp files removed. The harness must not
leave a listener or scratch file behind.

### Wave 4 — orchestrator only
Run the full workspace suite once, review and criticise each job's output,
verify `scan`, enrichment, and `open` end to end with `curl.exe` and the
browser, reconcile any findings into `docs/drift.md`, and clean up.

## Gate rules

- The orchestrator runs all gates. Never run `cargo build`, `cargo test`, or
  `cargo check` while other agents are writing the workspace.
- Smallest check first; full suite once at the end. A check that passed is not
  re-run.
- Rust commands run in WSL Debian; multiline logic goes in a `scripts/` file.
- Never put a server and a test in one command. Start detached, check the
  port separately, stop it after.
- Every wait is bounded; a long run is detached with a log outside the repo.
- A gap between the contract/spec and the code goes into `docs/drift.md`
  before the next edit. Challenge the contract; do not quietly please it.
- Each agent reports files written, public API added, tests added, and any
  deviation from the contract.
