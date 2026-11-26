# fnscope — what was verified

Plain-English record of the final checks. The orchestrator ran all of them in
WSL Debian on 2026-11-25 after the last fixes. This file reports results; it
does not restate the requirements (PRD-TRD.md) or the plan (IMPL.md).

## Checks and exact verdict markers

| What was checked | Command | Marker / result |
|---|---|---|
| Rust unit and integration tests | `cargo test --workspace`, `FNSCOPE_NODE` set to native Node 24 | `CARGO_TEST_STATUS=0`; 72 tests, 0 failures |
| Formatting | `cargo fmt --all -- --check` | `FMT_STATUS=0` |
| Full repo gate | `bash scripts/test-all.sh` | `ALL_TESTS_OK`, with `MANIFEST_OK: 9 functions, 8 call edges` and `SERVER_SMOKE_OK` |
| Enrichment cache end to end | `bash scripts/enrich-e2e.sh` | `ENRICH_E2E_OK`: 9 first-run calls, 0 unchanged, 0 formatting-only, new call on a semantic change |
| Real UI in a browser | release binary served on localhost; audit in `.reports/ui-browser.md` | four views, breadcrumb, 1-Hop Map and real Node invoke (`clamp -> 10`), no post-Run `simulated: true` |

The two tests that prove real Node execution are
`node_invocation_runs_a_real_export` and
`node_invocation_passes_a_scalar_parameter_by_name`. Both pass in the 72-test
run.

## What each result means

- The manifest is the source of truth. The manifest checker validated its
  schema and that every call edge resolves, finding 9 functions and 8 call
  edges.
- The server smoke check started `fnscope open` as a separate step from the
  tests, fetched every route, then stopped the server and freed the port.
- The enrichment check proves the cache is keyed on AST content: reformatting
  code makes no new API call, while changing a string does.
- The browser audit used the real served manifest, not the built-in demo
  graph. Its raw verdict was `PASS 7, FAIL 1, PARTIAL 1 (of 9 checks)`. The
  three findings it listed were sent back to J08 and J06, fixed, and the full
  suite and both harnesses were then re-run green.

## Known limits (do not read these as passes)

The limits that apply to both the base and the polyglot results are recorded
in "Polyglot verification" below: the Node runner executes only TS/JS exports,
constructors are listed as functions, and cross-file type resolution is best
effort. A simulated trace is not proof that project code ran.

## Where the evidence lives

- Drift status and resolution notes: `docs/drift.md`
- Gate-by-gate results: `docs/features/fnscope-base/PRD-TRD.md`
- Job ownership and review history: `docs/features/fnscope-base/IMPL.md`
- Raw browser audit: `.reports/ui-browser.md`
- Orchestrator iterations: `.reports/goal-build-fnscope.md`

## Polyglot verification

The orchestrator ran these checks in WSL Debian after J41-J43 integrated the
final adapter batch and resolved the Kotlin grammar conflict. These are
recorded results, not agent claims.

| What was checked | Command / action | Marker / result |
|---|---|---|
| Full workspace tests with all adapters | `cargo test --workspace` | 233 tests green (parser: 176) |
| 13-language mixed scan | real scan of the polyglot fixture | 78 functions, 159 steps, 33 resolved calls, 38 caller edges, 23 error facts, 0 dangling |
| Full repo gate | `bash scripts/test-all.sh` | `ALL_TESTS_OK` |
| Culture gate | `bash scripts/check-culture.sh` | `CULTURE_OK` |
| Drift closure | `docs/drift.md` | D-1 through D-6 `Resolved` |

The mixed scan covered `.ts`, `.js`, `.rs`, `.py`, `.go`, `.java`, `.c`,
`.cpp`, `.cs`, `.kt`, `.rb`, `.php` and `.swift`. Kotlin is parsed with
`tree-sitter-kotlin-ng`.

### Known limits (do not read these as passes)

- **The Node runner executes only TS/JS exports.** `--invoke node` imports a
  TypeScript/JavaScript module through Node. Rust, Python, Go, Java, C, C++,
  C#, Kotlin, Ruby, PHP and Swift functions are inspected and navigated, but
  invoking one returns the deterministic simulated trace. There is no executor
  for those languages. Node invocation is best effort: an incompatible Node
  falls back to a simulated trace with an `error` field instead of running the
  function.
- **Constructors are listed as functions.** Class constructors appear as
  function records alongside regular methods. That is expected, not a
  duplicate or a bug.
- **Cross-file type resolution is best effort.** Return fields are unpacked
  from aliases and interfaces declared inside the scanned set. Types imported
  from outside the scan, or from libraries, are not resolved into
  `returns.fields`.
