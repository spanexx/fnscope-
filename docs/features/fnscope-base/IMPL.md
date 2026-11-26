# fnscope-base — IMPL (implementation plan)

Status: SHIPPED (base TS/JS feature, Rust addendum J15-J18, adapter seam J21 and polyglot jobs J30-J44)

This maps the delivered jobs to files and to the gate each job had to pass.
The original eleven are listed in `.reports/goal-build-fnscope.md`
(lines 27-48); J12-J14 were added during verification (iterations 9-10). J15-J18
are the Rust addendum jobs, J21 is the adapter-seam refactor, and J30-J44 are
the Java/C/C++/Kotlin/C#/Ruby/PHP/Swift adapters, their reviews, registration,
the Kotlin grammar replacement and final docs closure. All are SHIPPED.
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
| J21 | `crates/fnscope-parser/**`, `crates/fnscope-core/**`, `docs/contract/adapter-api.md`, this file | language-adapter trait + registry; TS/JS and Rust grammar moved behind adapters; one registry lookup in core; `scan_with_registry` injection seam |
| J30 | `crates/fnscope-parser/src/java/`, `docs/contract/adapters/java.md` | Java adapter: 18 tests, wrapper-node handling, throws and type index |
| J31 | `crates/fnscope-parser/src/c_lang/`, `docs/contract/adapters/c.md` | C adapter: 11 tests |
| J32 | `crates/fnscope-parser/src/cpp/`, `docs/contract/adapters/cpp.md` | C++ adapter: 14 tests |
| J33 | `.reports/reviews/J30-J32.md` | independent review of Java/C/C++ -> PASS, 0 findings |
| J34 | registry wiring in `crates/fnscope-parser/src/adapter.rs`, `src/lib.rs`, manifests and core default globs | registered Java/C/C++ in `with_builtin_adapters` |
| J35 | `crates/fnscope-parser/src/kotlin/`, `docs/contract/adapters/kotlin.md` | Kotlin adapter; later ported to `tree-sitter-kotlin-ng` by J42 |
| J36 | `crates/fnscope-parser/src/csharp/`, `docs/contract/adapters/csharp.md` | C# adapter: primary constructors, records, local functions |
| J37 | `crates/fnscope-parser/src/ruby/`, `docs/contract/adapters/ruby.md` | Ruby adapter |
| J38 | `crates/fnscope-parser/src/php/`, `docs/contract/adapters/php.md` | PHP adapter: 16 tests |
| J39 | `crates/fnscope-parser/src/swift/`, `docs/contract/adapters/swift.md` | Swift adapter: class/struct/actor/extension/enum handling |
| J40 | `.reports/reviews/J35-J39.md` | strict review of the final five -> FAIL with 3 findings; Kotlin pointer bridge judged SOUND |
| J41 | registry wiring in `crates/fnscope-parser/src/adapter.rs`, `src/lib.rs`, manifests and core default globs | registered Kotlin/C#/Ruby/PHP/Swift in `with_builtin_adapters` |
| J42 | `crates/fnscope-parser/src/kotlin/` | replaced the raw-pointer bridge with the `tree-sitter-kotlin-ng` grammar |
| J43 | root `Cargo.toml`, `crates/fnscope-parser/Cargo.toml` | renamed the two grammar dependency lines |
| J44 | `README.md`, `docs/features/fnscope-base/{PRD-TRD,IMPL,VERIFY}.md`, four `docs/contract/adapters/*.md` | final docs closure: SHIPPED status, polyglot verification, stable F3 references |

The Rust addendum (J15-J18), the adapter seam (J21) and the polyglot jobs
(J30-J44) are all **SHIPPED**. The final gate recorded 233 green tests and a
13-language mixed scan; those results are in `VERIFY.md`. Jobs wrote files only
and the orchestrator reviewed each one before running any gate.

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

## Delivery notes (reconstructed 2026-11-26)

Dates, commit hashes and verdict markers below are taken from the orchestrator
ledger `.reports/goal-build-fnscope.md`; they are recorded observations, not
claims made by this documentation job. No cargo was run to write this section.

- **2026-11-25 - base build, iterations 1-14 (J01-J14).** Iteration 2 wrote the
  root workspace, contract, stubs and the eleven-job decomposition
  (`IMPL.md:16-53`, ledger lines 60-66). Gate history: iteration 3
  `CARGO_CHECK_STATUS=101` with 7 parser errors, returned to J02/J03; iteration
  4 one wrong model assertion, returned to J01; iteration 5 one wrong parser
  assertion, returned to J02; iterations 6-7 cross-environment Node path
  failures, returned to J06; iteration 8 `CARGO_TEST_STATUS=0` with 66 tests,
  `FMT_STATUS=0` and `ALL_TESTS_OK` (`MANIFEST_OK: 9 functions, 8 call edges`,
  `SERVER_SMOKE_OK`); iteration 9 `ENRICH_E2E_OK`; iteration 10 browser audit
  found 3 UI/API bugs; iterations 11-14 repaired and re-gated them. Close
  verdict at iteration 14 lines 156-168: `cargo fmt --all -- --check` -> 0,
  `cargo test --workspace` -> 0 with 72 tests, `ALL_TESTS_OK`,
  `ENRICH_E2E_OK`, browser re-audit 6 PASS / 0 FAIL, cleanup with no CRLF and
  no leftover ports or scratch dirs. No commit was made at this point; line
  168 records the commit as a human gate.

- **2026-11-26 - first committed record of the base, iteration 17.** Ledger
  lines 190-192: `git log --oneline` shows `ff793af docs: record verified base
  build and Rust addendum plan` and, on top of it, `4c26063 docs: turn build
  mistakes into enforceable ethics and workflows`. Verification marker on that
  batch: `CR_COUNT=0` after LF normalization (later exposed as a false green
  and closed as D-3). Caveat from `docs/STATUS.md:375-376`: a later history
  rewrite superseded these early hashes - `4c26063` is no longer present and
  the culture batch appears as `fa5419e`. Hashes here are quoted as the ledger
  recorded them at the time.

- **2026-11-26 - engine and Rust backend batch, iteration 20.** Ledger lines
  214-220: `a961c53 feat: fnscope engine, CLI, server, UI and Rust backend`
  (62 files, explicit staging). Markers: `RUST_GATE2_STATUS=0`, parser 35
  tests; real scan of `tests/fixtures/rust_app` -> 7 functions, 12 steps, 2
  calledBy edges, 0 dangling; real scan of `crates/` -> 188 functions, 722
  steps, 243 calledBy edges, 0 dangling.

- **2026-11-26 - culture gate batch, iteration 21.** Ledger lines 222-228:
  `ec2e8b0 docs: enforce reviewed-workflow culture and prove the culture
  gate`. Markers: `bash -n scripts/check-culture.sh` -> exit 0,
  `CULTURE_SELFTEST_OK`, eight `[PASS]` lines and `CULTURE_OK`; D-3 moved from
  Open to Resolved with the real command and output quoted in `docs/drift.md`.

- **2026-11-26 - adapter registry, iteration 24.** Ledger lines 246-252:
  `a6f5cde refactor: pluggable language adapter registry`. Markers:
  `ADAPTER_GATE_STATUS=0` with 101 tests (parser 44) and J22 re-review
  `Verdict (re-review): PASS` with 0 findings after D-4 was closed. Iteration
  23's earlier false PASS was kept in `.reports/reviews/J21.md` as evidence.

- **2026-11-26 - Python/Go batch, iteration 30.** Ledger lines 294-300:
  `01062af` (29 files). Markers: 127 tests green, `ALL_TESTS_OK`, `CULTURE_OK`,
  mixed scan of 28 functions across 5 extensions with 0 dangling edges, D-5
  Resolved.

- **2026-11-26 - Java/C/C++ batch, iteration 35.** Ledger lines 333-338:
  `5408cad`. Markers: gate green with 130 tests; 8-language mixed scan with
  46 functions, 105 steps, 19 resolved calls and 0 dangling edges.

- **2026-11-26 - final five adapters, iteration 42.** Ledger lines 387-393:
  `d6b02cf feat: Kotlin, C#, Ruby, PHP and Swift language adapters`. Markers:
  `ADAPTER_GATE_STATUS=0` with 233 tests (parser 176); 13-language scan with
  78 functions, 159 steps, 33 resolved calls, 38 caller edges, 23 error facts
  and 0 dangling edges; `CULTURE_OK`, `ALL_TESTS_OK`, D-6 Resolved.

- **2026-11-26 - delivery, iteration 43.** Ledger lines 395-401: `fbb10f9
  docs: record polyglot completion and current state` (11 files), then
  `git push -u origin main` -> `PUSH_STATUS=0` and `PUSH_VERIFIED`.

- **2026-11-26 - origin reconstruction (J47, this job).** Added the
  reconstructed decision record and human-gates note to `GRILL.txt`, the
  behavioral and simulation contract sections to `PRD-TRD.md`, this delivery
  section, and the canonical and pre-implementation simulation scripts in this
  folder. This job wrote documentation only: no cargo, no gate claim, no
  commit, and no edit outside `docs/features/fnscope-base/`.
