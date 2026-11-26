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

- **Node invocation is best effort.** With `--invoke node`, fnscope tries a
  real Node export. If the chosen Node is missing or too old to import `.ts`
  files, it falls back to a simulated trace and adds an `error` field saying
  why. The request still succeeds, so a simulation is not proof that code ran.
- **Constructors are listed as functions.** Class constructors appear as
  function records, so the sample fixture's function list includes
  `constructor`. That is expected, not a duplicate or a bug.
- **Cross-file type resolution is bounded.** Return fields are unpacked from
  aliases and interfaces declared inside the scanned set. Types imported from
  outside the scan, or from libraries, are not resolved into `returns.fields`.

## Where the evidence lives

- Drift status and resolution notes: `docs/drift.md`
- Gate-by-gate results: `docs/features/fnscope-base/PRD-TRD.md`
- Job ownership and review history: `docs/features/fnscope-base/IMPL.md`
- Raw browser audit: `.reports/ui-browser.md`
- Orchestrator iterations: `.reports/goal-build-fnscope.md`

## Rust addendum (pending)

The Rust source support in J15-J18 is not verified yet. The orchestrator will
run the checks below in WSL Debian after integrating and reviewing each job.
Nothing in this section is a recorded result: every verdict is `PENDING` until
the orchestrator replaces it with evidence.

| What will be checked | Command / action | Verdict |
|---|---|---|
| Workspace tests with the Rust backend | `cargo test --workspace` | `PENDING` |
| Real scan of the Rust fixture | `fnscope scan tests/fixtures/rust_app` and inspect the emitted `fn-graph.json` (functions found, schema valid, deterministic steps and `calledBy` edges) | `PENDING` |
| Real scan of the Rust source tree | `fnscope scan crates` and confirm `.rs` files are discovered, `target/` is excluded, and functions are extracted into the same schema | `PENDING` |

No result is claimed for Rust invocation: `--invoke node` remains a TS/JS
runner and does not execute Rust code.
