# fnscope — Current state (updated 2026-11-27)

> **Workspace UI update (2026-11-27).** The explorer described in the 2026-11-26 baseline below
> has been replaced by the validated workspace UI: isolated project scopes, an entry-point/module
> explorer, Story / 1-Hop / Split / Raw views, a call-path breadcrumb and a real invoke drawer.
> Evidence: `docs/features/workspace-ui/` (GRILL, PRD-TRD, IMPL, `simulate.sh`),
> `.reports/ui-workspace-audit.md` and `.reports/ui-workspace-audit/` screenshots.
> `bash scripts/test-all.sh` prints `ALL_TESTS_OK`, `bash docs/features/workspace-ui/simulate.sh`
> prints `WORKSPACE_SIM_OK`, and `bash scripts/check-culture.sh` prints `CULTURE_OK`.
> Workspace routes are additive: `GET /api/scopes`, `GET /api/scopes/<id>/fn-graph.json`,
> `POST /api/scopes/<id>/invoke`; the legacy routes keep serving the default scope.
> Live Node invoke remains opt-in per scope and is not exercised by the simulate-only fixture;
> deep links (Scenario 13) remain deferred.

This file is the single entry point for a human who wants to know where the build
actually is. Every number and marker below is quoted from a file in this repo,
with the path and (where useful) the line. The workspace-UI gates were re-run on
2026-11-27 at code HEAD `cefc072` (this status refresh is `8a0ffbe`); the older
base sections quote recorded evidence rather than a fresh run.

Evidence sources: `README.md`, `AGENTS.md`, `.reports/goal-build-fnscope.md`
(the 42-iteration base ledger), `.reports/goal-workspace-ui.md` (the 11
workspace-UI iterations), `docs/drift.md`,
`docs/features/fnscope-base/{GRILL.txt,PRD-TRD.md,IMPL.md,VERIFY.md}`,
`docs/features/workspace-ui/{GRILL-workspace-ui.txt,PRD-TRD-workspace-ui.md,IMPL-workspace-ui.md,VERIFY-workspace-ui.md,simulate.sh}`,
`.reports/ui-workspace-audit.md`, `docs/concepts/language-adapter.md`,
`docs/contract/{adapter-api,rust-api}.md`, `docs/workflows/reviewer-process.md`,
`docs/ethics/lessons.md`, `.reports/reviews/*.md`, `.reports/ui-browser.md`,
`Cargo.toml`, `crates/fnscope-parser/src/adapter.rs`,
`crates/fnscope-core/src/scan.rs`, and a read-only
`git log --oneline -12` / `git status --short --untracked-files=all` taken in
WSL Debian while writing this file.

## TL;DR

- fnscope is built end to end in Rust: a local-first, single binary with the
  React explorer compiled into it and no account, cloud service or telemetry
  (`README.md:3-5`, `README.md:159-167`). There are six workspace crates plus
  `ui/` (`Cargo.toml:1-9`).
- The work spans 42 recorded base iterations (2026-11-25 to 2026-11-26,
  `.reports/goal-build-fnscope.md`) plus 11 workspace-UI iterations
  (2026-11-27, `.reports/goal-workspace-ui.md`). Failures, pivots and verdicts
  are all still in both ledgers, including false greens that were later
  corrected.
- **Coverage, stated precisely.** `README.md:5` and `:18` now say "13 named
  languages … across 16 file suffixes", matching the code. The four numbers
  measure different things: 13 named languages (TypeScript covers `.ts`/`.tsx`
  and JavaScript covers `.js`/`.jsx`), 16 default suffixes
  (`crates/fnscope-parser/src/adapter.rs:128-145`;
  `crates/fnscope-core/src/scan.rs:29-46`), 15 adapter instances registered
  (`adapter.rs:78-94`, with four `TypeScriptAdapter` instances and a C++
  adapter claiming both `.cpp` and `.cc`), and 13 extensions in the mixed
  polyglot *scan that actually ran* — `.ts`, `.js`, `.rs`, `.py`, `.go`,
  `.java`, `.c`, `.cpp`, `.cs`, `.kt`, `.rb`, `.php`, `.swift`
  (`.reports/goal-build-fnscope.md:389`).
- Final gate on workspace code HEAD `cefc072`: `cargo test --workspace` -> **270
  passed, 0 failed**, of which 176 are parser tests, 26 CLI, 26 server. The
  base 233-test figure below is the 2026-11-26 baseline, before the workspace
  server/CLI tests landed. The 13-extension mixed scan still produced 78
  functions, 159 steps, 33 resolved calls, 38 caller edges, 23 error facts and
  0 dangling edges (`.reports/goal-build-fnscope.md`;
  `docs/features/fnscope-base/VERIFY.md`).
- Drift D-1 through D-12 are all `Resolved` (`docs/drift.md`). D-7 (Preview ->
  Story) and D-9…D-12 (mobile view control, CSP inline script, literal
  `\u00b7`, fixture call edge) closed on 2026-11-27 with the workspace UI. The
  eleven lessons L1–L11 are enforced by
  `scripts/check-culture.sh --self-test`, the reviewer checklist and the
  orchestrator's compile gate (`docs/ethics/lessons.md`).
- Recorded gate markers on the final workspace tree: `ALL_TESTS_OK`,
  `MANIFEST_OK: 9 functions, 8 call edges`, `SERVER_SMOKE_OK`,
  `WORKSPACE_SIM_OK`, `UI_CONTRACT_OK` (2 functions for the workspace fixture),
  `UI_CHECK_OK (52 source files)`, `CULTURE_SELFTEST_OK`, `CULTURE_OK`,
  `ENRICH_E2E_OK` and `ADAPTER_GATE_STATUS=0`
  (`docs/features/workspace-ui/VERIFY-workspace-ui.md`, `README.md`).
- Real workspace browser audit: 12 scenarios PASS/PARTIAL, 1 deferred (deep
  links), with 375/768/1440 measurements and screenshots
  (`.reports/ui-workspace-audit.md`, `.reports/ui-workspace-audit/`). The
  earlier single-scope audit (7 PASS / 1 FAIL / 1 PARTIAL, then 6 PASS / 0 FAIL)
  remains historical evidence in `.reports/ui-browser.md`.
- Committed in discrete logical batches: 44 commits on `feature/workspace-ui`
  over `main` at `69998d2`. The latest code commit is `cefc072`; this status
  refresh is the only commit after it. The working tree is clean and no docs
  batch is pending. `main` is untouched; the human PR/merge gate is the only
  remaining step.

## What the tool does today

fnscope scans a source tree and builds a deterministic function graph:
signatures, ordered execution steps, upstream callers and downstream calls,
purity and permissions, throws, and a content hash per function
(`README.md:3-5`; `docs/features/fnscope-base/PRD-TRD.md:40-47`). Structural
truth comes from tree-sitter AST parsing; an LLM is only an optional,
hash-cached enrichment pass (`PRD-TRD.md:11-19`). The graph is served through a
local web explorer with isolated project scopes and four views — Story, 1-Hop,
Split and Raw — plus a sandbox invoke drawer (`README.md:5`,
`docs/features/workspace-ui/PRD-TRD-workspace-ui.md:21-124`).

Two commands cover the product surface, and `open` can serve one project or a
workspace of isolated scopes:

- `fnscope scan <TARGET_DIR>` writes the graph and, optionally, enriched text
  to `.fnscope/`.
- `fnscope open <TARGET_DIR>` (or a bare target) scans once, serves the UI on
  `127.0.0.1:4310`, and watches for file changes (`README.md:51-97`;
  `docs/contract/rust-api.md:203-210`).
- `fnscope open --workspace fnscope.workspace.json` scans each configured scope
  into `.fnscope/scopes/<id>/fn-graph.json` and serves `/api/scopes`,
  `/api/scopes/<id>/fn-graph.json` and `/api/scopes/<id>/invoke`. Without a
  workspace config the single-scope behavior is unchanged.

The base TS/JS feature and the polyglot addendum are both marked `SHIPPED` in
`docs/features/fnscope-base/PRD-TRD.md:3` and `:221-227`. `IMPL.md:3-10` marks
the Rust addendum (J15–J18), the adapter seam (J21) and the polyglot jobs
(J30–J44) as shipped as well.

## Architecture and module boundaries

The workspace has six crates (`Cargo.toml:2-9`), each with one job:

| Crate | Owns | Evidence |
| --- | --- | --- |
| `fnscope-model` | The shared JSON schema (`FnGraph`, `FnRecord`, `Step`, `EnrichCache`, invoke types), camelCase serde renames and `FnGraph::validate` | `README.md:160`; `docs/contract/rust-api.md:7-92` |
| `fnscope-parser` | One grammar-owning adapter per language behind `LanguageAdapter`; returns only contract shapes | `README.md:161`; `crates/fnscope-parser/src/adapter.rs:27-58` |
| `fnscope-core` | File walk, ids, `ast_hash`, name-to-id edges, reverse `called_by`, manifest I/O — no language knowledge | `README.md:162`; `docs/concepts/language-adapter.md:48-65` |
| `fnscope-enrich` | Hash-cached LLM enrichment, mock/HTTP/noop backends, cache read/write | `README.md:163`; `docs/contract/rust-api.md:159-176` |
| `fnscope-server` | axum routes, `include_str!`-embedded UI assets, `/api/invoke` in simulate or node mode | `README.md:164`; `docs/contract/rust-api.md:180-201` |
| `fnscope-cli` | clap `scan` / `open`, 500 ms polling watcher, progress and errors | `README.md:165`; `docs/contract/rust-api.md:203-210` |

`ui/` is the React explorer; its built files in `ui/dist/` are embedded into
the binary at compile time, so there is no runtime Node requirement
(`README.md:166`; `docs/contract/rust-api.md:187-189`).

**The seam.** `LanguageAdapter` is six methods — `id`, `extensions`,
`discover`, `analyze`, `collect_types`, `resolve_return_fields`
(`crates/fnscope-parser/src/adapter.rs:33-58`). `Registry::with_builtin_adapters`
registers 15 adapter instances in stable order, and
`Registry::for_extension` trims a leading dot and lowercases the suffix before
picking the first claimant (`adapter.rs:64-118`). Core does one lookup per file
and calls `collect_types`, `discover`, `resolve_return_fields` and `analyze`;
ids, hashes, edges and manifest assembly are untouched. `scan_with_registry`
is the single injection seam that lets a caller supply a private registry
(`docs/contract/adapter-api.md:78-100`). The old free functions
`discover` / `analyze` / `collect_types` / `resolve_return_fields` remain as
thin delegates for compatibility (`adapter-api.md:47-61`).

There are 12 adapter module directories under
`crates/fnscope-parser/src/`: `typescript`, `rust`, `python`, `golang`, `java`,
`c_lang`, `cpp`, `csharp`, `kotlin`, `php`, `ruby`, `swift`. The four TS-family
registrations are four `TypeScriptAdapter` instances so each suffix keeps its
original grammar (TypeScript vs TSX vs JavaScript)
(`docs/contract/adapter-api.md:37-41`, `:65-69`).

`Lang` still exists in `crates/fnscope-parser/src/types.rs` only so old
callers compile; core no longer imports or matches on it
(`docs/concepts/language-adapter.md:113-117`). The rule for any new language
is: implement the trait, register it, add fixtures/tests — never edit the core
graph builder, schema, enricher or UI (`docs/concepts/language-adapter.md:67-73`,
`:103-111`).

## Language coverage (exact table)

Suffixes, modules and ids are from `crates/fnscope-parser/src/adapter.rs:128-145`
and `:78-94`. Test counts are the current `#[test]` attributes counted
read-only from source on 2026-11-26; the "shared" rows point at the same module
directory, so they must not be added up as separate tests.

| Suffix | Adapter module | Adapter `id()` | `#[test]` in module dir |
| --- | --- | --- | --- |
| `.ts` | `typescript` (`TypeScriptAdapter(Lang::TypeScript)`) | `typescript` | 2 (shared) |
| `.tsx` | `typescript` (`TypeScriptAdapter(Lang::Tsx)`) | `tsx` | 2 (shared) |
| `.js` | `typescript` (`TypeScriptAdapter(Lang::JavaScript)`) | `javascript` | 2 (shared) |
| `.jsx` | `typescript` (`TypeScriptAdapter(Lang::Jsx)`) | `jsx` | 2 (shared) |
| `.rs` | `rust` | `rust` | 2 |
| `.py` | `python` | `python` | 10 |
| `.go` | `golang` | `go` | 16 |
| `.java` | `java` | `java` | 18 |
| `.c` | `c_lang` | `c` | 12 |
| `.cpp`, `.cc` | `cpp` | `cpp` | 14 |
| `.cs` | `csharp` | `csharp` | 13 |
| `.kt` | `kotlin` | `kotlin` | 14 |
| `.rb` | `ruby` | `ruby` | 9 |
| `.php` | `php` | `php` | 16 |
| `.swift` | `swift` | `swift` | 10 |

Totals: 136 tests across the 12 module directories, plus 38 original TS/JS
tests kept byte-for-byte in the compatibility files `discover.rs` (22) and
`body.rs` (16), plus 2 registry tests in `adapter.rs` = 176 parser tests
(`.reports/reviews/J21.md:21-22`, `:65-66`). The workspace total is 233
(`.reports/goal-build-fnscope.md:389`). The independent reviews counted 11 C
tests and 9 Swift tests at review time
(`.reports/reviews/J30-J32.md:42`, `.reports/reviews/J35-J39.md:135`); current
source has 12 and 10, and F1 (the Swift count) was routed back to J39 per
iteration 36 (`.reports/goal-build-fnscope.md:341-345`).

Kotlin uses `tree-sitter-kotlin-ng 1.1`; PHP uses `tree_sitter_php::LANGUAGE_PHP`
rather than `LANGUAGE` (`Cargo.toml:37`; `.reports/reviews/J35-J39.md:120-121`).
The grammar versions pinned in `Cargo.toml:30-43` are `tree-sitter 0.25`, C
0.24.2, C# 0.24, C++ 0.23.4, Go 0.25, Java 0.23.5, JavaScript 0.25,
Kotlin-ng 1.1, PHP 0.25, Python 0.25, Ruby 0.23.1, Rust 0.24, Swift 0.7.4 and
TypeScript 0.23.

## CLI and artifacts

Exact flags from `--help` are reproduced in `README.md:31-98`.

`fnscope scan [OPTIONS] <TARGET_DIR>`:

| Flag | Meaning |
| --- | --- |
| `--out <OUT>` | output directory, default `.fnscope` |
| `--enrich` | enrich using an LLM plus the local cache |
| `--model <MODEL>` | model name, defaults to the configured low-cost model |
| `--include <INCLUDE>` | include glob, repeatable |
| `--exclude <EXCLUDE>` | exclude glob, repeatable |

`fnscope open [OPTIONS] [TARGET_DIR]`:

| Flag | Meaning |
| --- | --- |
| `--port <PORT>` | default `4310` |
| `--host <HOST>` | default `127.0.0.1` |
| `--no-watch` | disable the live file watcher |
| `--invoke <INVOKE>` | `simulate` (default) or `node` |
| `--workspace <FILE>` | serve the scopes listed in a `fnscope.workspace.json` |
| `--scope <ID=PATH>` | add or override one scope; repeatable |
| `--default-scope <ID>` | pick the scope used by the compatibility routes |
| `--enrich`, `--model` | enrichment controls |

`fnscope <TARGET>` is shorthand for `fnscope open <TARGET>`, and top-level
flags work in both forms (`README.md:69`; `docs/contract/rust-api.md:205-210`).

Default includes are 16 globs — `**/*.ts`, `**/*.tsx`, `**/*.js`, `**/*.jsx`,
`**/*.rs`, `**/*.py`, `**/*.go`, `**/*.java`, `**/*.c`, `**/*.cpp`, `**/*.cc`,
`**/*.cs`, `**/*.kt`, `**/*.php`, `**/*.rb`, `**/*.swift`
(`crates/fnscope-core/src/scan.rs:29-46`; `README.md:49`). Default excludes are
10 globs: `node_modules`, `dist`, `**/target/**`, `**/.fnscope/**`,
`**/__pycache__/**`, the `.test.ts` / `.test.tsx` / `.spec.ts` / `.spec.tsx`
patterns and `**/.*/**` (`scan.rs:50-63`).

Artifacts under `.fnscope/` (`README.md:121-130`):

- `fn-graph.json` — the full graph: functions, parameters, steps, return
  shapes, errors, call edges and source code. This is what the UI reads.
- `scopes/<id>/fn-graph.json` — one manifest per configured workspace scope,
  written only in workspace mode.
- `enrich-cache.json` — LLM cache keyed by each function's `astHash`.

`astHash` is `sha256:<64 hex>` over source with comments and all whitespace
outside string/template literals stripped (`docs/contract/rust-api.md:152-154`;
`README.md:130`).

## Enrichment and caching

Enrichment is off by default; add `--enrich` to `scan` or `open`
(`README.md:131-141`). The first non-empty key among `OPENAI_API_KEY`,
`GEMINI_API_KEY` and `ANTHROPIC_API_KEY` is used; `FNSCOPE_LLM_BASE_URL`
points at a chat-completions endpoint (a plain `http://127.0.0.1:<port>/...`
mock is supported) and `FNSCOPE_LLM_MODEL` overrides the default model, with
`--model NAME` taking precedence (`README.md:135-139`).

Cache behaviour: key = `fn.ast_hash`. A hit overwrites description, step
titles/details and parameter defaults with zero API calls; a miss sends only
the compact AST skeleton and stores the reply under that hash
(`docs/contract/rust-api.md:169-178`; `PRD-TRD.md:140-150`). With no key,
`--enrich` warns and falls back to deterministic text with zero API calls;
`--model NAME` without a key exits with a readable error (`README.md:172`).
`NoopEnricher` is the deterministic zero-call fallback and `MockBackend` counts
calls for tests (`docs/contract/rust-api.md:163-167`).

Recorded proof: `bash scripts/enrich-e2e.sh` -> `ENRICH_E2E_OK` with 9 mock
calls on the first scan, 0 on an unchanged rescan, 0 after a formatting-only
change and a new call after a semantic edit
(`docs/features/fnscope-base/VERIFY.md:14`, `:29-30`;
`.reports/goal-build-fnscope.md:118`, `:162`).

## Invoke semantics — what is real and what is simulated

`POST /api/invoke` takes `{ fnId, args }` and returns
`{ ok, data, error, trace, durationMs }` (`docs/contract/rust-api.md:78-87`,
`:192-201`).

- **`simulate` (default)** builds one `TraceEntry` per step with
  `status: "simulated"` and `output: step.output`, plus
  `data: { simulated: true, function, inputs, expectedReturnType }`. It never
  runs project code (`README.md:174`; `docs/contract/rust-api.md:200`).
- **`node`** writes a temporary runner, imports the target TS/JS module and
  calls the named export (class methods as `Class.method`), passing the module
  path and arguments as argv rather than embedding a `file:///` URL. On any
  failure it falls back to the simulated trace and adds an `error` field; the
  request does not fail (`README.md:143-150`; `docs/contract/rust-api.md:201`;
  `.reports/goal-build-fnscope.md:92-106`).

Real execution was verified with native Node 24 selected through
`FNSCOPE_NODE`: the two tests `node_invocation_runs_a_real_export` and
`node_invocation_passes_a_scalar_parameter_by_name` pass
(`docs/features/fnscope-base/VERIFY.md:17-20`), and the browser captured
`clamp -> 10`, an 8-token array for `tokenize`, `slugify -> hello-world` and a
flat-args `roleId: doer` result, with no post-Run `simulated: true`
(`.reports/ui-browser.md:243-285`, `:287`). The default WSL `node` on PATH was
v20.20.2 and could not import `.ts`; Windows Node was v24.20.0
(`docs/drift.md:27-31`). That environment gap is D-1 and is Resolved.

**Only TypeScript/JavaScript are executed.** For Rust, Python, Go, Java, C,
C++, C#, Kotlin, Ruby, PHP and Swift there is no executor: invoking those
functions returns the deterministic simulated trace (usually with an `error`
field explaining the fallback) (`README.md:170`; `PRD-TRD.md:250-257`;
`docs/features/fnscope-base/VERIFY.md:71-77`). The `runtime` labels `universal`
and `node20` are carried data describing the recorded target environment, not
a promise that Node can run that language (`README.md:172`).

## Local server and UI

The server exposes the compatibility routes plus the additive workspace routes
(`docs/contract/rust-api.md`):

| Route | Result |
| --- | --- |
| `GET /` | `INDEX_HTML` as `text/html` |
| `GET /theme.js` | `THEME_JS` as `text/javascript` |
| `GET /app.js` | `APP_JS` as `text/javascript` |
| `GET /app.css` | `APP_CSS` as `text/css` |
| `GET /api/scopes` | scope descriptors and `defaultScope` |
| `GET /api/scopes/<id>/fn-graph.json` | that scope's validated manifest |
| `POST /api/scopes/<id>/invoke` | scoped `InvokeRequest` -> `InvokeResponse` |
| `GET /api/fn-graph.json` | default-scope manifest, `404` if missing, never panic |
| `POST /api/invoke` | default-scope invoke |
| `GET /api/health` | `{"ok":true,"tool":"fnscope"}` |

`router(config)` is testable without binding; `serve(config)` binds and runs.
The four UI assets are embedded with `include_str!`. The workspace explorer has
Story, 1-Hop, Split and Raw views, an entry-point/module explorer with search,
a call-path breadcrumb and an invoke drawer that labels the server's mode
honestly (`docs/features/workspace-ui/PRD-TRD-workspace-ui.md:21-124`).

Historical browser evidence below (`.reports/ui-browser.md`) describes the
retired single-scope explorer; the workspace UI evidence is
`.reports/ui-workspace-audit.md` and the screenshots under
`.reports/ui-workspace-audit/`.

Browser evidence (`.reports/ui-browser.md`) used the release binary against the
real served manifest, not the built-in demo data:

- First audit: `PASS 7, FAIL 1, PARTIAL 1 (of 9 checks)` (`:188-190`). The three
  bugs were (1) `.json` raw view omitted `tool` / `schemaVersion` /
  `generatedAt` / `rootDir`, (2) `Jump ↑` truncated the breadcrumb and removed
  `Back`, (3) the sandbox arg contract mismatched for object and scalar
  parameters (`:180-186`).
- Targeted re-audit on a fresh binary: `PASS 6, FAIL 0` for R1–R3d (`:289-290`).
  Real metadata appeared in `.json`, `Back` survived `Jump ↑`, and the default
  invoke sent a repaired nested `input` object (`:198-263`).
- Minor observations left in the audit: duplicate breadcrumb nodes after
  `Jump ↑` then `Back` (cosmetic), an empty `"trace":{}` in Node-mode responses,
  and a pre-Run `simulated: true` placeholder that is not an invoke response
  (`:292-295`). Iteration 14 subsequently merged the deterministic trace into
  Node-mode responses (`.reports/goal-build-fnscope.md:157-163`); no later
  browser capture of the trace field is recorded in the repo.

## Verification evidence (exact commands and markers)

All rows are recorded results quoted from the named files. None of these
commands was re-run for this status file.

| What was checked | Command | Marker / result | Source |
| --- | --- | --- | --- |
| Formatting | `cargo fmt --all -- --check` | `FMT_STATUS=0` | `docs/features/fnscope-base/VERIFY.md:12` |
| Workspace tests (base, with native Node 24) | `cargo test --workspace`, `FNSCOPE_NODE` set | `CARGO_TEST_STATUS=0`; 72 tests, 0 failures | `VERIFY.md:11`, `:17-20` |
| Workspace tests (final polyglot gate) | `cargo test --workspace` | `ADAPTER_GATE_STATUS=0`; 233 tests (parser 176) | `.reports/goal-build-fnscope.md:387-389`; `VERIFY.md:59` |
| Full repo gate | `bash scripts/test-all.sh` | `ALL_TESTS_OK`; `MANIFEST_OK: 9 functions, 8 call edges`; `SERVER_SMOKE_OK` | `docs/features/workspace-ui/VERIFY-workspace-ui.md`; `README.md` |
| Workspace simulation | `bash docs/features/workspace-ui/simulate.sh` | `WORKSPACE_SIM_OK`; `UI_CONTRACT_OK (2 functions)` | `docs/features/workspace-ui/VERIFY-workspace-ui.md` |
| Workspace tests | `cargo test --workspace` | 270 passed, 0 failed (parser 176, CLI 26, server 26) | code HEAD `cefc072` |
| UI source / dist | `node ui/scripts/check-ui.mjs` | `UI_CHECK_OK (52 source files)` | final HEAD |
| UI helper tests | `npm --prefix ui test` | 5 pass, 0 fail | final HEAD |
| Workspace browser audit | in-app browser + headless Chrome CDP | 12 PASS/PARTIAL, deep links deferred; 375/768/1440 no overflow | `.reports/ui-workspace-audit.md` |
| Enrichment end to end | `bash scripts/enrich-e2e.sh` | `ENRICH_E2E_OK`: 9 first-run calls, 0 unchanged, 0 formatting-only, new call on semantic change | `VERIFY.md:14`; `README.md:156` |
| Culture gate | `bash scripts/check-culture.sh --self-test` then `bash scripts/check-culture.sh` | `CULTURE_SELFTEST_OK`, then eight `[PASS]` lines and `CULTURE_OK` | `.reports/goal-build-fnscope.md:224` |
| Culture gate (final) | `bash scripts/check-culture.sh` | `CULTURE_OK` | `.reports/goal-build-fnscope.md:389`; `VERIFY.md:62` |
| 13-extension mixed scan | real scan of the polyglot fixture | 78 functions, 159 steps, 33 resolved calls, 38 caller edges, 23 error facts, 0 dangling | `.reports/goal-build-fnscope.md:389`; `VERIFY.md:60` |
| Real UI in a browser | release binary served on localhost; audit in `.reports/ui-browser.md` | first 7 PASS / 1 FAIL / 1 PARTIAL, 3 bugs; re-audit 6 PASS / 0 FAIL | `.reports/ui-browser.md:188-190`, `:289-290` |
| Drift closure | `docs/drift.md` | D-1 through D-12 `Resolved` | `docs/drift.md` |
| This file's line endings | `tr -cd '\r' < docs/STATUS.md \| wc -c` | `CR_COUNT=0` (checked 2026-11-26 with a single-quoted script) | this document |

Intermediate gate failures are also recorded, not hidden: `CARGO_CHECK_STATUS=101`
with 7 errors (iteration 3), `ADAPTER_GATE_STATUS=101` at iterations 23, 27,
28, 33, 34, 37, 38, 39, 40 and 41, and `RUST_GATE_STATUS=101` at iteration 18.
The final green at iteration 42 is the current verdict
(`.reports/goal-build-fnscope.md:68-74`, `:198-204`, `:238-245`, `:270-293`,
`:317-385`, `:387-393`).

## Commits so far (from git log)

The branch has 44 commits over `main` at `69998d2` (`docs:`). The base build's
first nine commits are history; the workspace-UI wave on top is listed after
them. The latest code commit is `cefc072`; this status refresh sits on top of
it.

Base history (2026-11-25/26), quoted from the base ledger:

```text
d6b02cf feat:
5408cad feat:
01062af feat:
a6f5cde refactor:
ec2e8b0 docs:
a961c53 feat:
fa5419e docs:
ff793af docs:
8835585 initial commit
```

Workspace-UI wave (2026-11-27), newest first:

```text
cefc072 docs(workspace-ui): record the final integration review
8eb6a11 fix(ui): let Escape close only the mobile view menu
89a858e docs(workspace-ui): record the audit, verify and drift closure
8c4f1d4 test(cli): prove two-scope call edges stay isolated
d3cf48a fix(ui): keep palette bootstrap in the CSP and finish mobile polish
7f8e057 fix(cli): waive test-only absolute path
47f22ca feat(ui): wire explorer, views and drawer into the shell
f6dcb08 feat(cli): add workspace scope configuration
c4f97f4 feat(ui): add invoke drawer with real trace
f2c0bb0 fix(ui): remove duplicate callee parentheses
8cdff1c feat(ui): add story, 1-hop, split and raw views
a893f92 feat(ui): add entry-point and module explorer
19a089c chore(guardrails): cover scope routes and waive test path
86f9a09 fix(server): kill timed-out node process tree
b0334ae feat(server): add workspace scope registry and scoped routes
...
```

The working tree is clean. Unlike the 2026-11-26 baseline, there is no
uncommitted docs batch; the human PR/merge gate is the only remaining step.

| Commit | Date | Shortstat | What the batch added |
| --- | --- | --- | --- |
| `8835585` | 2026-11-25 | 2 files, +2053 | Initial spec and UI contract: `fnscope_architecture_agent_build_spec.md` (234 lines) and `fnscope_ui.tsx` (1819 lines) — from the `git show --stat` output for `8835585` |
| `ff793af` | 2026-11-26 | 17 files, +1810 | Recorded the verified base build and the Rust addendum plan (ledger iteration 17) |
| `fa5419e` | 2026-11-26 | 15 files, +1032 / -209 | Turned build mistakes into enforceable ethics/concepts/workflows (J19); iteration 17 cites `4c26063` for this batch, but that hash is absent from current history |
| `a961c53` | 2026-11-26 | 62 files, +15238 | Engine, CLI, server, embedded UI and Rust backend (J01–J18); ledger iteration 20 |
| `ec2e8b0` | 2026-11-26 | 13 files, +817 / -216 | Enforced the reviewed-workflow culture and proved the culture gate (J20); iteration 21 |
| `a6f5cde` | 2026-11-26 | 21 files, +4264 / -2266 | Pluggable language-adapter registry and `scan_with_registry` (J21/J22); iteration 24 |
| `01062af` | 2026-11-26 | 29 files, +4184 / -38 | Python and Go adapters, registration and fixes (J23/J24/J26/J27); iteration 30 |
| `5408cad` | 2026-11-26 | 26 files, +6568 / -6 | Java, C and C++ adapters (J30–J34); iteration 35 |
| `d6b02cf` | 2026-11-26 | 39 files, +11404 / -1 | Kotlin, C#, Ruby, PHP and Swift adapters, Kotlin grammar replacement and registration (J35–J43); iteration 42 |

Two honest notes about this history. First, the actual commit subjects are only
the conventional prefixes (`feat:`, `docs:`, `refactor:`); the fuller subjects
in `.reports/goal-build-fnscope.md` describe the batches, not the git subject
lines. Second, the **base** history has eight conventional commits after the
initial commit, not six, and the ledger's `4c26063` is no longer present — the
culture batch now appears as `fa5419e`. The workspace-UI wave uses descriptive
subjects and is listed separately above.

The base table above describes the 2026-11-26 history; its commit subjects are
minimal because the descriptive subjects live in the base ledger. The
workspace-UI wave uses descriptive conventional subjects and is fully committed.
`git status --short --untracked-files=all` is empty on this status refresh.

## Decisions and drift (D-1..D-12, L1..L11 summary)

Founding decisions, all made under delegated authority ("ask me no questions
make all decisions yourself") and recorded at `.reports/goal-build-fnscope.md:19-25`:

- **D1** Rust instead of Node/TypeScript; tree-sitter replaces the spec's
  ts-morph.
- **D2** Build lane is WSL Debian (Rust 1.99.1, gcc 14.2.0); Windows is the same
  files at the WSL mount.
- **D3** UI stays React, compiled at build time and embedded in the binary.
- **D4** Real functions via Node runner when available, else a deterministic
  simulated trace.
- **D5** Watcher polls a directory fingerprint every 500 ms instead of using a
  native watcher crate.

Drift entries (all `Resolved`; full text and `Verified by:` notes in
`docs/drift.md`):

| ID | Gap | Resolution evidence (short) |
| --- | --- | --- |
| D-1 | Fixture needs Node 24 type stripping; WSL PATH had Node v20.20.2 while Windows had v24.20.0 | `FNSCOPE_NODE` override plus a version-aware test skip; native Node 24 verified with two real-invoke tests and the browser audit (`drift.md:27-31`) |
| D-2 | `evaluate-grammar-role.returns.fields == []` because discovery resolved aliases only in the same file | Project-wide type index in core; real scan now shows `roleId`, `label`, `confidence`, `intuition` from `src/types.ts` (`drift.md:33-37`) |
| D-3 | Ledger's `CR_COUNT=0` was a false green from `$"\r"` (literal backslash-r) while six ethics files still had CR bytes | Corrected with `grep -rlU $'\r' … \| wc -l` and `sed -i 's/\r$//'`; then `tr -cd '\r' … \| wc -c` = 0 for all ten files (`drift.md:39-43`) |
| D-4 | A CR-normalization command treated `\r$` as a literal trailing `r`, truncating identifiers; `rustfmt --check` and a reviewer PASS were both false | Re-merged 13 files with `cp` only, 0 CR bytes, `ADAPTER_GATE_STATUS=0` with 101 tests, and a documented re-review PASS (`drift.md:45-49`; `.reports/reviews/J21.md:38-79`) |
| D-5 | Go adapter doc mapped `short_var_declaration` to Transform even when the initializer calls a function | `step_kind()` returns Call when `has_call()` is true, doc corrected, gate green at 127 tests (`drift.md:51-55`) |
| D-6 | `tree-sitter-kotlin` 0.3.8 pins tree-sitter <0.23 and cannot coexist with workspace 0.25 under `links = "tree-sitter"`, despite a sound pointer bridge | Replaced with `tree-sitter-kotlin-ng 1.1`, removed all unsafe bridge code, 14 Kotlin tests preserved, 233-test gate green (`drift.md:57-61`) |
| D-7 | README/base PRD promised a Preview view; the validated design calls the primary view Story | Story rendered and audited; README, base PRD and Rust PRD renamed; dead `FunctionExplorer.tsx` removed (`drift.md`) |
| D-8 | Server comment promised graph validation before serving | `FnGraph::validate()` called before serving; corrupt-manifest test proves the 500 + `available:false` behavior |
| D-9 | PRD requires the view switcher to stay reachable at narrow widths | Compact mobile view menu below 640px; CDP measured all header controls inside 375px with no overflow |
| D-10 | `script-src 'self'` forbade the inline palette bootstrap | External `/theme.js` served from an embedded asset; console clean, theme still persists |
| D-11 | `\u00b7` in JSX text rendered literally in two components | Both use `{'\u00b7'}`; browser footer and scope card read correctly |
| D-12 | Fixture claimed a call edge the TypeScript parser never records for `return` statements | Fixtures call the helper in a declaration; `shared_call_edge_stays_inside_its_scope` and `WORKSPACE_SIM_OK` prove it |

Lessons L1–L11, one line each from `docs/ethics/lessons.md:13-27`:

- **L1** Iteration 3 — seven compiler errors from parallel writes; bound write windows and follow with a compile window; agents never run cargo while peers write.
- **L2** Iterations 4/5/11 — three bad assertions and one missing field; triage CODE vs TEST against the locked contract before editing.
- **L3** Iterations 6–8 — WSL/Windows Node path translation; use shared reachable paths, argv instead of embedded strings, and a native runtime.
- **L4** Iteration 10 — sandbox arg contract mismatch; audit the payload at every boundary with captured requests.
- **L5** Iteration 10 — breadcrumb and `.json` view passed source review but failed the browser; rendered behavior needs a real browser check.
- **L6** Iteration 13 — a drift status cell said `Open` while its note said `Resolved`; machine-readable status fields are the gate.
- **L7** Iteration 12 — a concern was raised after the fix landed; verify against current code and cite the line before escalating.
- **L8** Iterations 13–14 — empty `trace` in Node responses; a response field the UI renders is part of the API contract.
- **L9** Iteration 16 — polyglot expansion; a new language is a plugin, not a branch in the graph builder.
- **L10** Iteration 17 — `CR_COUNT=0` from `$"\r"`, a false green; prove a check against known positive and negative controls, and put byte checks in a script.
- **L11** Iteration 23 — `ALL_OWNED_RS_FMT_OK` from `rustfmt --check`, then 12 `E0425` errors; a format check is not a correctness check, and byte normalization must use `tr -d '\r'`, never `sed 's/\r$//'`.

The loop that enforces these is `docs/workflows/reviewer-process.md`: every
implementation brief names owned paths, carries the verbatim independent-review
notice and the 12-item checklist, and forbids cargo and commits. Reviewers write
`.reports/reviews/<job>.md` with PASS or FAIL and do not run gates
(`reviewer-process.md:26-56`, `:66-97`). A false PASS is kept as evidence, not
deleted (`:89-97`).

Independent review reports present: `.reports/reviews/J21.md` (false PASS, then
re-review PASS with 0 findings), `J23-J24.md` (FAIL, 10 findings, Go clean),
`J30-J32.md` (PASS, 0 findings), `J35-J39.md` (FAIL, 3 findings; Kotlin pointer
bridge judged sound), and `lang-batch-triage.md` (CODE 7, TEST 1, BOTH 1).

## Known limits and honest caveats

- **Workspace UI limits.** IBM Plex fonts are not bundled yet, so the UI falls
  back to system fonts while staying fully offline (no CDN). Deep links
  (`#/scope/<id>/fn/<fnId>/<view>`) are deferred by PRD Scenario 13. The
  two-scope fixture is simulate-only, so live Node invoke is covered by server
  tests and the earlier single-scope audit, not by the workspace browser audit.
  A zero-function scope was not produced for a screenshot; the shared empty
  state is source-reviewed. The caller-jump breadcrumb keeps the validated
  prototype behavior (prepend the caller, last crumb stays the previous
  function).
- **Only TS/JS are executed.** `--invoke node` is a TypeScript/JavaScript
  runner. The other languages are inspected and navigated but invoking one
  returns the simulated trace (`README.md:170`; `PRD-TRD.md:250-257`).
- **Node invocation is best effort.** An incompatible Node falls back to a
  simulated trace with an `error` field instead of failing the request
  (`README.md:150`, `:143`).
- **Constructors are listed as functions** alongside regular methods
  (`README.md:174`; `docs/features/fnscope-base/VERIFY.md:78-80`).
- **Cross-file type resolution is best effort.** Return fields are unpacked
  only from aliases, interfaces, structs and enums declared inside the scanned
  set; imported or external types fall back to their written names
  (`README.md:173`; `VERIFY.md:81-84`).
- **Kotlin uses `tree-sitter-kotlin-ng`**, not the older `tree-sitter-kotlin`
  that pins an incompatible tree-sitter (`README.md:171`; `drift.md:57-61`).
  The Swift (0.7.4), PHP (0.25) and Kotlin-ng (1.1) grammar crates are older
  pins but were compiled and tested in the final gate (`Cargo.toml:37-42`;
  `.reports/goal-build-fnscope.md:387-389`).
- **Runtime labels are data, not executors.** `universal` and `node20` are
  recorded environment labels for every language (`README.md:172`).
- **Counting conventions, not a discrepancy.** `README.md:5` and `:18` now state
  16 default suffixes, agreeing with `scan.rs:29-46` and `adapter.rs:128-145`;
  the old "15 file suffixes (plus `.cc` …)" wording is gone. The figures still
  refer to different things: 13 named languages (TypeScript and JavaScript each
  cover two suffixes), 16 suffixes, 15 registered adapter instances, and 13
  extensions in the mixed scan that actually ran. No `docs/drift.md` entry was
  needed because the README and the code now agree.
- **Commit subjects are minimal.** `git log --oneline` shows only `feat:` /
  `docs:` / `refactor:`; the descriptive subjects live in the ledger.
- **No pending commit.** The 2026-11-26 docs batch was committed; the workspace
  wave is committed through `cefc072`, plus this status refresh; the tree is clean.
- **Gate results on this HEAD were re-run**, not only quoted: 270 workspace
  tests, `ALL_TESTS_OK`, `UI_CHECK_OK`, `WORKSPACE_SIM_OK` and `CULTURE_OK`.
  Older rows in the table above remain quoted base evidence.
- **Browser evidence is current for the workspace UI.** The trace field is
  covered by the 2026-11-27 audit (simulate run with a real trace and duration);
  the older empty-`trace` observation remains historical.

## What remains / next steps

1. **Human PR/merge gate.** Everything is committed on `feature/workspace-ui`
   at `cefc072` plus this status refresh; `main` is untouched and nothing was pushed. The PR body can
   cite `docs/features/workspace-ui/VERIFY-workspace-ui.md` and
   `.reports/ui-workspace-audit.md`.
2. Optional polish: bundle IBM Plex (OFL) as local WOFF2 assets and record the
   built-size/offline check; add a simulate/live two-scope fixture pair.
3. Optional Scenario 13: hash deep links (`#/scope/<id>/fn/<fnId>/<view>`) with
   browser Back, only if the core stays green.
4. Optional capability work: add another adapter by implementing
   `LanguageAdapter`, registering it and adding fixtures/tests; no core/UI
   changes (`docs/concepts/language-adapter.md`,
   `docs/contract/adapter-api.md`).
5. No further commits or tags without the human go; agents do not merge or push
   (`AGENTS.md`).

## How to run it yourself

From the repository root in WSL Debian (build in WSL; the built binary can also
be run from PowerShell):

```bash
cargo build --release -p fnscope
./target/release/fnscope scan ./tests/fixtures/sample_app/src
./target/release/fnscope ./tests/fixtures/sample_app/src
./target/release/fnscope ./tests/fixtures/rust_app/src
./target/release/fnscope open --workspace tests/fixtures/workspace/fnscope.workspace.json
```

Then open <http://localhost:4310> and press Ctrl-C to stop the server. The
second and third commands are the bare-target form of `open`: scan once, serve
the UI and watch for changes. The fourth serves the two-scope Alpha/Beta
fixture so the scope switcher and the scoped invoke routes are visible
(`README.md`).

From PowerShell, the same steps are (`cargo` is not on the Windows PATH here):

```powershell
wsl.exe -d Debian --exec bash -lc 'cargo build --release -p fnscope'
.\target\release\fnscope.exe scan .\tests\fixtures\sample_app\src
.\target\release\fnscope.exe .\tests\fixtures\sample_app\src
.\target\release\fnscope.exe .\tests\fixtures\rust_app\src
```

(`README.md:20-29`)

For real TS/JS invocation rather than the simulated trace, set `FNSCOPE_NODE`
to a Node 24 executable and serve with `--invoke node`
(`README.md:143-150`; `docs/drift.md:29-31`).

Verification harnesses, run in WSL and not while agents are writing crate
files (`README.md:151-158`):

```bash
bash scripts/test-all.sh
bash scripts/enrich-e2e.sh
bash scripts/check-culture.sh --self-test
bash scripts/check-culture.sh
```
